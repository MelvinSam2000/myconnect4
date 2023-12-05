use std::sync::Arc;
use std::time::Duration;

use opentelemetry::global;
use opentelemetry::trace::Span;
use opentelemetry::trace::Tracer;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio::time::sleep;
use tokio::time::Instant;

use super::BUFFER_MAX;
use crate::actor::HB_SEND_DUR;

const DEFAULT_WAIT_LIMIT: Duration = Duration::from_millis(10000);

#[derive(Debug)]
pub enum MessageIn {
    CancelSearch {
        user: String,
    },
    Poll,
    Search {
        user: String,
    },
    QueryGetState {
        respond_to: oneshot::Sender<StatePayload>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageOut {
    HeartBeat,
    UsersFound { users: (String, String) },
    LongWait { user: String },
}

struct UserRecord {
    created_at: Instant,
    user: String,
}

#[derive(Debug)]
pub struct StatePayload {
    pub queue: Vec<String>,
}

pub struct MatchMakingActor {
    state: ActorState,
    rx_in: Receiver<MessageIn>,
}

#[derive(Clone)]
struct ActorState {
    tx_in: Sender<MessageIn>,
    tx_out: Sender<MessageOut>,
    queue: Arc<RwLock<Vec<UserRecord>>>,
    wait_limit: Duration,
}

#[derive(Debug, Error)]
enum ActorSendError {
    #[error("Error sending msg: {0}")]
    MessageIn(#[from] SendError<MessageIn>),
    #[error("Error sending msg: {0}")]
    MessageOut(#[from] SendError<MessageOut>),
}

impl MatchMakingActor {
    pub fn new(tx_out: Sender<MessageOut>, wait_limit: Option<Duration>) -> Self {
        let (tx_in, rx_in) = mpsc::channel(BUFFER_MAX);
        let wait_limit = wait_limit.unwrap_or(DEFAULT_WAIT_LIMIT);
        let queue = Arc::new(RwLock::new(vec![]));
        Self {
            state: ActorState {
                tx_in,
                tx_out,
                queue,
                wait_limit,
            },
            rx_in,
        }
    }

    pub fn get_sender(&self) -> Sender<MessageIn> {
        self.state.tx_in.clone()
    }

    pub fn start(self) {
        let MatchMakingActor { state, mut rx_in } = self;

        log::debug!("MatchMaking actor started.");
        let mut tasks = JoinSet::new();

        // listen for incoming events
        let state_1 = state.clone();
        tasks.spawn(async move {
            let state = state_1;
            let tracer = global::tracer("MM Actor");
            while let Some(msg) = rx_in.recv().await {
                let mut span = tracer.start(format!("MM RECV {msg:?}"));
                log::debug!("RECV {msg:?}");
                if let Err(e) = Self::handle_msg_in(&state, msg).await {
                    log::error!("{e}");
                }
                span.end();
            }
        });

        // polling/long wait check task
        let state_2 = state.clone();
        let wait_limit = state.wait_limit;
        tasks.spawn(async move {
            let state = state_2;
            loop {
                sleep(wait_limit).await;
                log::debug!("POLL");
                if state.tx_in.send(MessageIn::Poll).await.is_err() {
                    log::error!("Could not send MessageIn::Poll");
                }

                let rqueue = state.queue.read().await;
                let users_delayed: Vec<String> = rqueue
                    .iter()
                    .filter(|record| record.created_at.elapsed() > wait_limit)
                    .map(|record| &record.user)
                    .cloned()
                    .collect();
                drop(rqueue);
                for user in users_delayed {
                    if state
                        .tx_out
                        .send(MessageOut::LongWait { user })
                        .await
                        .is_err()
                    {
                        log::error!("Could not send MessageOut::LongWait");
                    };
                }
            }
        });

        // send heartbeat
        let state_3 = state.clone();
        tasks.spawn(async move {
            let state = state_3;
            loop {
                sleep(HB_SEND_DUR).await;
                if let Err(e) = state.tx_out.send(MessageOut::HeartBeat).await {
                    log::error!("Could not send HB: {e}");
                }
            }
        });

        tokio::spawn(async move {
            if let Some(_) = tasks.join_next().await {
                log::error!("Terminating MatchMaking Actor");
                tasks.abort_all();
            }
        });
    }

    async fn handle_msg_in(state: &ActorState, msg: MessageIn) -> Result<(), ActorSendError> {
        match msg {
            MessageIn::Search { user } => {
                let rqueue = state.queue.read().await;
                if rqueue.iter().any(|record| user == record.user) {
                    log::warn!("Duplicate user {user} trying to search. Ignoring");
                    return Ok(());
                }
                drop(rqueue);
                let mut wqueue = state.queue.write().await;
                wqueue.push(UserRecord {
                    created_at: Instant::now(),
                    user,
                });
                drop(wqueue);

                if let Some(users) = Self::poll_for_users(&state.queue).await {
                    state.tx_out.send(MessageOut::UsersFound { users }).await?;
                }
            }
            MessageIn::Poll => {
                if let Some(users) = Self::poll_for_users(&state.queue).await {
                    state.tx_out.send(MessageOut::UsersFound { users }).await?;
                }
            }
            MessageIn::CancelSearch { user } => {
                let rqueue = state.queue.read().await;
                if rqueue.iter().any(|record| user == record.user) {
                    drop(rqueue);
                    state.queue.write().await.retain(|u| u.user != user);
                }
            }
            MessageIn::QueryGetState { respond_to } => {
                let rqueue = state.queue.read().await;
                let queue = rqueue.iter().map(|record| &record.user).cloned().collect();
                drop(rqueue);
                if respond_to.send(StatePayload { queue }).is_err() {
                    log::error!("Failed to respond back to query");
                }
            }
        }
        Ok(())
    }

    async fn poll_for_users(queue: &RwLock<Vec<UserRecord>>) -> Option<(String, String)> {
        let rqueue = queue.read().await;
        if rqueue.len() < 2 {
            return None;
        }
        drop(rqueue);
        let mut wqueue = queue.write().await;
        if wqueue.len() < 2 {
            return None;
        }
        let user1 = wqueue.pop().expect("queue should have len > 2");
        let user2 = wqueue.pop().expect("queue should have len > 2");
        Some((user2.user, user1.user))
    }
}
