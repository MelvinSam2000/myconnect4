use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::RwLock;

use super::game::MessageRequest as GReq;
use super::game::MessageResponse as GRes;
use super::matchmaking::MatchMakingActor;
use super::matchmaking::MessageRequest as MMReq;
use super::matchmaking::MessageResponse as MMRes;
use super::BUFFER_MAX;
use crate::actor;
use crate::actor::game::GameActor;

#[derive(Debug)]
pub enum MessageRequest {
    SearchGame,
    Move {
        col: usize,
    },
    UserLeft,
    QueryGetState {
        respond_to: oneshot::Sender<StatePayload>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageResponse {
    NewGame {
        game_id: u64,
        rival: String,
        first_turn: bool,
    },
    MoveValid {
        valid: bool,
    },
    RivalMove {
        col: usize,
    },
    GameOver {
        won: Option<bool>,
    },
    RivalLeft,
}

#[derive(Debug)]
pub struct StatePayload {
    _mm_state: actor::matchmaking::StatePayload,
    _g_state: actor::game::StatePayload,
    _c_state: ControllerStatePayload,
}

#[derive(Debug)]
pub struct ControllerStatePayload {
    pub users: HashSet<String>,
}

pub struct MainControllerActor {
    tx_mm_req: mpsc::Sender<MMReq>,
    rx_mm_res: mpsc::Receiver<MMRes>,
    tx_g_req: mpsc::Sender<GReq>,
    rx_g_res: mpsc::Receiver<GRes>,
    tx_req: mpsc::Sender<(String, MessageRequest)>,
    rx_req: mpsc::Receiver<(String, MessageRequest)>,
    map_tx_resp: Arc<RwLock<HashMap<String, mpsc::Sender<MessageResponse>>>>,
}

impl MainControllerActor {
    pub fn new() -> Self {
        let (tx_mm_res, rx_mm_res) = mpsc::channel(BUFFER_MAX);
        let mmactor = MatchMakingActor::new(tx_mm_res, None);
        let tx_mm_req = mmactor.get_sender();
        mmactor.start();

        let (tx_g_res, rx_g_res) = mpsc::channel(BUFFER_MAX);
        let gactor = GameActor::new(tx_g_res);
        let tx_g_req = gactor.get_sender();
        gactor.start();

        let (tx, rx) = mpsc::channel(BUFFER_MAX);

        Self {
            tx_mm_req,
            rx_mm_res,
            tx_g_req,
            rx_g_res,
            tx_req: tx,
            rx_req: rx,
            map_tx_resp: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_sender(&self) -> mpsc::Sender<(String, MessageRequest)> {
        self.tx_req.clone()
    }

    pub fn get_tx_map(&self) -> Arc<RwLock<HashMap<String, mpsc::Sender<MessageResponse>>>> {
        self.map_tx_resp.clone()
    }

    pub fn start(mut self) {
        log::debug!("Controller actor started.");
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some((user, msg)) = self.rx_req.recv() => {
                        log::debug!("RECV {msg:?} from {user}");
                        self.handle_controller_req(user, msg).await;
                    }
                    Some(msg) = self.rx_mm_res.recv() => {
                        log::debug!("RECV {msg:?} from MM actor");
                        self.handle_mmres(msg).await;
                    },
                    Some(msg) = self.rx_g_res.recv() => {
                        log::debug!("RECV {msg:?} from G actor");
                        self.handle_gres(msg).await;
                    }
                }
            }
        });
    }

    async fn handle_controller_req(&self, user: String, msg: MessageRequest) {
        match msg {
            MessageRequest::SearchGame => {
                self.tx_mm_req.send(MMReq::Search { user }).await.unwrap();
            }
            MessageRequest::Move { col } => {
                self.tx_g_req.send(GReq::Move { user, col }).await.unwrap();
            }
            MessageRequest::UserLeft => {
                let mut wmap = self.map_tx_resp.write().await;
                wmap.remove(&user).unwrap();
                drop(wmap);
                self.tx_g_req
                    .send(GReq::UserLeft { user: user.clone() })
                    .await
                    .unwrap();
                self.tx_mm_req
                    .send(MMReq::CancelSearch { user })
                    .await
                    .unwrap();
            }
            MessageRequest::QueryGetState { respond_to } => {
                let (tx_mm, rx_mm) = oneshot::channel();
                let (tx_g, rx_g) = oneshot::channel();
                self.tx_mm_req
                    .send(MMReq::QueryGetState { respond_to: tx_mm })
                    .await
                    .unwrap();
                self.tx_g_req
                    .send(GReq::QueryGetState { respond_to: tx_g })
                    .await
                    .unwrap();
                let rmap = self.map_tx_resp.read().await;
                let users = rmap.keys().cloned().collect::<HashSet<String>>();
                drop(rmap);
                let mm_state = rx_mm.await.unwrap();
                let g_state = rx_g.await.unwrap();
                let state = StatePayload {
                    _mm_state: mm_state,
                    _g_state: g_state,
                    _c_state: ControllerStatePayload { users },
                };
                respond_to.send(state).unwrap();
            }
        }
    }

    async fn handle_mmres(&self, msg: MMRes) {
        match msg {
            MMRes::UsersFound { users } => {
                self.tx_g_req.send(GReq::NewGame { users }).await.unwrap();
            }
            MMRes::LongWait { user: _ } => {
                log::warn!("Ignoring msg.");
            }
        }
    }

    async fn handle_gres(&self, msg: GRes) {
        match msg {
            GRes::NewGame {
                game_id,
                users,
                first_turn,
            } => {
                let (user0, user1) = users;
                let map_tx_resp = self.map_tx_resp.read().await;
                let tx0 = map_tx_resp.get(&user0).unwrap();
                let tx1 = map_tx_resp.get(&user1).unwrap();
                tx0.send(MessageResponse::NewGame {
                    game_id,
                    rival: user1.clone(),
                    first_turn: first_turn == user0,
                })
                .await
                .unwrap();
                tx1.send(MessageResponse::NewGame {
                    game_id,
                    rival: user0.clone(),
                    first_turn: first_turn == user1,
                })
                .await
                .unwrap();
            }
            GRes::UserAlreadyInGame {
                reject_users,
                rematchmake_users,
            } => {
                for user in reject_users {
                    log::warn!("User {user} tried to matchmake but is already in a game.");
                }
                for user in rematchmake_users {
                    self.tx_mm_req.send(MMReq::Search { user }).await.unwrap();
                }
            }
            GRes::UserNotInGame { user } => {
                log::warn!("User {user} tried to make a MOVE event, but he is not in a game.");
            }
            GRes::MoveValid {
                player,
                rival,
                valid,
                col,
            } => {
                let tx_map = self.map_tx_resp.read().await;
                if valid {
                    tx_map
                        .get(&player)
                        .unwrap()
                        .send(MessageResponse::MoveValid { valid: true })
                        .await
                        .unwrap();
                    tx_map
                        .get(&rival)
                        .unwrap()
                        .send(MessageResponse::RivalMove { col })
                        .await
                        .unwrap();
                } else {
                    tx_map
                        .get(&player)
                        .unwrap()
                        .send(MessageResponse::MoveValid { valid: false })
                        .await
                        .unwrap();
                }
            }
            GRes::GameOverWinner { winner, loser } => {
                let tx_map = self.map_tx_resp.read().await;
                let winner_tx = tx_map.get(&winner).unwrap();
                let loser_tx = tx_map.get(&loser).unwrap();
                winner_tx
                    .send(MessageResponse::GameOver { won: Some(true) })
                    .await
                    .unwrap();
                loser_tx
                    .send(MessageResponse::GameOver { won: Some(false) })
                    .await
                    .unwrap();
            }
            GRes::GameOverDraw { users } => {
                let tx_map = self.map_tx_resp.read().await;
                let user0_tx = tx_map.get(&users.0).unwrap();
                let user1_tx = tx_map.get(&users.1).unwrap();
                user0_tx
                    .send(MessageResponse::GameOver { won: None })
                    .await
                    .unwrap();
                user1_tx
                    .send(MessageResponse::GameOver { won: None })
                    .await
                    .unwrap();
            }
            GRes::AbortGame { user } => {
                self.map_tx_resp
                    .read()
                    .await
                    .get(&user)
                    .unwrap()
                    .send(MessageResponse::RivalLeft)
                    .await
                    .unwrap();
            }
        }
    }
}
