use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio::time::Instant;

use super::BUFFER_MAX;

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
    tx_in: mpsc::Sender<MessageIn>,
    tx_out: mpsc::Sender<MessageOut>,
    rx: mpsc::Receiver<MessageIn>,
    queue: Arc<RwLock<Vec<UserRecord>>>,
    wait_limit: Duration,
}

impl MatchMakingActor {
    pub fn new(tx_out: mpsc::Sender<MessageOut>, wait_limit: Option<Duration>) -> Self {
        let (tx, rx) = mpsc::channel(BUFFER_MAX);
        let wait_limit = wait_limit.unwrap_or(DEFAULT_WAIT_LIMIT);
        Self {
            tx_in: tx,
            tx_out,
            rx,
            queue: Arc::new(RwLock::new(Vec::new())),
            wait_limit,
        }
    }

    pub fn get_sender(&self) -> mpsc::Sender<MessageIn> {
        self.tx_in.clone()
    }

    pub fn start(mut self) {
        let tx_in = self.tx_in.clone();
        let tx_out = self.tx_out.clone();
        let queue = self.queue.clone();
        let wait_limit = self.wait_limit;
        log::debug!("MatchMaking actor started.");
        // main task
        tokio::spawn(async move {
            while let Some(req) = self.rx.recv().await {
                log::debug!("RECV {req:?}");
                match req {
                    MessageIn::Search { user } => {
                        let rqueue = self.queue.read().await;
                        if rqueue.iter().any(|record| user == record.user) {
                            log::warn!("Duplicate user {user} trying to search. Ignoring");
                            continue;
                        }
                        drop(rqueue);
                        let mut wqueue = self.queue.write().await;
                        wqueue.push(UserRecord {
                            created_at: Instant::now(),
                            user,
                        });
                        drop(wqueue);

                        if let Some(users) = Self::poll_for_users(&self.queue).await {
                            self.tx_out
                                .send(MessageOut::UsersFound { users })
                                .await
                                .unwrap();
                        }
                    }
                    MessageIn::Poll => {
                        if let Some(users) = Self::poll_for_users(&self.queue).await {
                            self.tx_out
                                .send(MessageOut::UsersFound { users })
                                .await
                                .unwrap();
                        }
                    }
                    MessageIn::CancelSearch { user } => {
                        let rqueue = self.queue.read().await;
                        if rqueue.iter().any(|record| user == record.user) {
                            drop(rqueue);
                            self.queue.write().await.retain(|u| u.user != user);
                        }
                    }
                    MessageIn::QueryGetState { respond_to } => {
                        let rqueue = self.queue.read().await;
                        let queue = rqueue.iter().map(|record| &record.user).cloned().collect();
                        if let Err(_) = respond_to.send(StatePayload { queue }) {
                            log::error!("Failed to respond back to query");
                        }
                    }
                }
            }
        });
        // polling/delay check task
        tokio::spawn(async move {
            loop {
                sleep(wait_limit).await;
                tx_in.send(MessageIn::Poll).await.unwrap();
                let rqueue = queue.read().await;
                let users_delayed: Vec<String> = rqueue
                    .iter()
                    .filter(|record| record.created_at.elapsed() > wait_limit)
                    .map(|record| &record.user)
                    .cloned()
                    .collect();
                drop(rqueue);
                for user in users_delayed {
                    tx_out.send(MessageOut::LongWait { user }).await.unwrap();
                }
            }
        });
    }

    async fn poll_for_users(queue: &RwLock<Vec<UserRecord>>) -> Option<(String, String)> {
        let rqueue = queue.read().await;
        if rqueue.len() < 2 {
            return None;
        }
        drop(rqueue);
        let mut wqueue = queue.write().await;
        let user1 = wqueue.pop().unwrap();
        let user2 = wqueue.pop().unwrap();
        Some((user2.user, user1.user))
    }
}
