use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::RwLock;

const BUFFER_MAX: usize = 100;

#[derive(Debug)]
pub enum MessageRequest {
    Search { user: String },
    CancelSearch { user: String },
    Poll,
    DebugGetQueue,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageResponse {
    UsersFound { users: (String, String) },
    LongWait { user: String },
    DebugGetQueueResponse { queue: Vec<String> },
}

pub struct MatchMakingActor {
    tx_req: mpsc::Sender<MessageRequest>,
    tx_res: mpsc::Sender<MessageResponse>,
    rx: mpsc::Receiver<MessageRequest>,
    queue: Arc<RwLock<Vec<String>>>,
}

impl MatchMakingActor {
    pub fn new(tx_res: mpsc::Sender<MessageResponse>) -> Self {
        let (tx, rx) = mpsc::channel(BUFFER_MAX);
        Self {
            tx_req: tx,
            tx_res,
            rx,
            queue: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn get_sender(&self) -> mpsc::Sender<MessageRequest> {
        self.tx_req.clone()
    }

    pub fn start(mut self) {
        tokio::spawn(async move {
            while let Some(req) = self.rx.recv().await {
                log::debug!("RECV {req:?}");
                match req {
                    MessageRequest::Search { user } => {
                        let rqueue = self.queue.read().await;
                        if rqueue.contains(&user) {
                            log::warn!("Duplicate user {user} trying to search. Ignoring");
                            continue;
                        }
                        drop(rqueue);
                        let mut wqueue = self.queue.write().await;
                        wqueue.push(user);
                        drop(wqueue);

                        if let Some(users) = Self::poll_for_users(&self.queue).await {
                            self.tx_res
                                .send(MessageResponse::UsersFound { users })
                                .await
                                .unwrap();
                        }
                    }
                    MessageRequest::Poll => {
                        if let Some(users) = Self::poll_for_users(&self.queue).await {
                            self.tx_res
                                .send(MessageResponse::UsersFound { users })
                                .await
                                .unwrap();
                        }
                    }
                    MessageRequest::CancelSearch { user } => {
                        let rqueue = self.queue.read().await;
                        if rqueue.contains(&user) {
                            drop(rqueue);
                            self.queue.write().await.retain(|u| u != &user);
                        }
                    }
                    MessageRequest::DebugGetQueue => {
                        let rqueue = self.queue.read().await;
                        let queue = rqueue.clone();
                        drop(rqueue);
                        self.tx_res
                            .send(MessageResponse::DebugGetQueueResponse { queue })
                            .await
                            .unwrap();
                    }
                }
            }
        });
    }

    async fn poll_for_users(queue: &RwLock<Vec<String>>) -> Option<(String, String)> {
        let rqueue = queue.read().await;
        if rqueue.len() < 2 {
            return None;
        }
        drop(rqueue);
        let mut wqueue = queue.write().await;
        let user1 = wqueue.pop().unwrap();
        let user2 = wqueue.pop().unwrap();
        Some((user2, user1))
    }
}

#[tokio::test]
async fn test_normal_matchmaking_flow() {
    let (tx, mut rx) = mpsc::channel(BUFFER_MAX);
    let mmactor = MatchMakingActor::new(tx);
    let tx = mmactor.get_sender();
    mmactor.start();

    let users = ("Alice".to_string(), "Bob".to_string());

    tx.send(MessageRequest::Search {
        user: users.0.clone(),
    })
    .await
    .unwrap();

    tx.send(MessageRequest::DebugGetQueue).await.unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageResponse::DebugGetQueueResponse {
            queue: vec!["Alice".to_string()]
        })
    );

    tx.send(MessageRequest::Search {
        user: users.1.clone(),
    })
    .await
    .unwrap();
    let resp = rx.recv().await;
    assert_eq!(resp, Some(MessageResponse::UsersFound { users }));

    tx.send(MessageRequest::DebugGetQueue).await.unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageResponse::DebugGetQueueResponse { queue: vec![] })
    );
}

#[tokio::test]
async fn test_cancel_search_flow() {
    let (tx, mut rx) = mpsc::channel(BUFFER_MAX);
    let mmactor = MatchMakingActor::new(tx);
    let tx = mmactor.get_sender();
    mmactor.start();

    let user = String::from("Alice");

    tx.send(MessageRequest::Search { user: user.clone() })
        .await
        .unwrap();
    tx.send(MessageRequest::CancelSearch { user })
        .await
        .unwrap();
    tx.send(MessageRequest::DebugGetQueue).await.unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageResponse::DebugGetQueueResponse { queue: vec![] })
    );
}
