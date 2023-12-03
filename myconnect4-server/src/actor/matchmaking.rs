use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio::time::Instant;

use super::BUFFER_MAX;

const DEFAULT_WAIT_LIMIT: Duration = Duration::from_millis(10000);

#[derive(Debug)]
pub enum MessageIn {
    HeartBeat {
        respond_to: oneshot::Sender<()>,
    },
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
    rx_in: mpsc::Receiver<MessageIn>,
}

#[derive(Clone)]
struct ActorState {
    tx_in: mpsc::Sender<MessageIn>,
    tx_out: mpsc::Sender<MessageOut>,
    queue: Arc<RwLock<Vec<UserRecord>>>,
    wait_limit: Duration,
}

#[derive(Debug, Error)]
enum ActorSendError {
    #[error("Error sending msg: {0}")]
    MessageIn(#[from] SendError<MessageIn>),
    #[error("Error sending msg: {0}")]
    MessageOut(#[from] SendError<MessageOut>),
    #[error("Error sending oneshot")]
    Oneshot,
}

impl MatchMakingActor {
    pub fn new(tx_out: mpsc::Sender<MessageOut>, wait_limit: Option<Duration>) -> Self {
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

    pub fn get_sender(&self) -> mpsc::Sender<MessageIn> {
        self.state.tx_in.clone()
    }

    pub fn start(self) {
        let MatchMakingActor { state, mut rx_in } = self;

        log::debug!("MatchMaking actor started.");

        // listen for incoming events
        let state_1 = state.clone();
        tokio::spawn(async move {
            let state = state_1;
            while let Some(msg) = rx_in.recv().await {
                log::debug!("RECV {msg:?}");
                if let Err(e) = Self::handle_msg_in(&state, msg).await {
                    log::error!("{e}");
                }
            }
        });

        // polling/long wait check task
        let state_2 = state.clone();
        let wait_limit = state.wait_limit;
        tokio::spawn(async move {
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
    }

    async fn handle_msg_in(state: &ActorState, msg: MessageIn) -> Result<(), ActorSendError> {
        match msg {
            MessageIn::HeartBeat { respond_to } => {
                respond_to.send(()).map_err(|_| ActorSendError::Oneshot)?;
            }
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
                if let Err(_) = respond_to.send(StatePayload { queue }) {
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
