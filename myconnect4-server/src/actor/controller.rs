use std::collections::HashMap;
use std::sync::Arc;

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

use super::botmanager as bm;
use super::game as g;
use super::matchmaking as mm;
use super::matchmaking::MatchMakingActor;
use super::service as s;
use super::service::ServiceActor;
use super::BUFFER_MAX;
use crate::actor::botmanager::BotManagerActor;
use crate::actor::game::GameActor;
use crate::actor::HB_RECV_WAIT_LIMIT;

pub struct ActorController {
    state: ActorState,

    rx_mm_out: Receiver<mm::MessageOut>,
    mm_actor: MatchMakingActor,

    rx_g_out: Receiver<g::MessageOut>,
    g_actor: GameActor,

    rx_s_out: Receiver<s::MessageOut>,
    s_actor: Option<ServiceActor>,

    rx_bm_out: Receiver<bm::MessageOut>,
    rx_bms_out: Receiver<s::MessageOut>,
    bm_actor: BotManagerActor,
}

#[derive(Clone)]
struct ActorState {
    tx_mm_in: Sender<mm::MessageIn>,
    tx_g_in: Sender<g::MessageIn>,
    tx_s_in: Sender<s::MessageIn>,
    tx_bm_in: Sender<bm::MessageIn>,
    tx_bms_in: Sender<s::MessageIn>,
    map_hb_times: Arc<RwLock<HashMap<ActorType, Instant>>>,
}

#[derive(Debug, Error)]
enum ActorSendError {
    #[error("Error sending matchmaking msg: {0}")]
    MatchmakingMessageIn(#[from] SendError<mm::MessageIn>),
    #[error("Error sending matchmaking msg: {0}")]
    MatchmakingMessageOut(#[from] SendError<mm::MessageOut>),
    #[error("Error sending game msg: {0}")]
    GameMessageIn(#[from] SendError<g::MessageIn>),
    #[error("Error sending game msg: {0}")]
    GameMessageOut(#[from] SendError<g::MessageOut>),
    #[error("Error sending service msg: {0}")]
    ServiceMessageIn(#[from] SendError<s::MessageIn>),
    #[error("Error sending service msg: {0}")]
    ServiceMessageOut(#[from] SendError<s::MessageOut>),
    #[error("Error sending bot manager msg: {0}")]
    BotManagerMessageIn(#[from] SendError<bm::MessageIn>),
    #[error("Error sending bot manager msg: {0}")]
    BotManagerMessageOut(#[from] SendError<bm::MessageOut>),
    #[error("Error sending oneshot")]
    Oneshot,
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum ActorType {
    MatchMaking,
    Game,
    Service,
    BotManager,
}

impl ActorController {
    pub fn new() -> Self {
        let (tx_mm_out, rx_mm_out) = mpsc::channel(BUFFER_MAX);
        let mm_actor = mm::MatchMakingActor::new(tx_mm_out, None);
        let tx_mm_in = mm_actor.get_sender();

        let (tx_g_out, rx_g_out) = mpsc::channel(BUFFER_MAX);
        let g_actor = GameActor::new(tx_g_out);
        let tx_g_in = g_actor.get_sender();

        let (tx_s_out, rx_s_out) = mpsc::channel(BUFFER_MAX);
        let s_actor = ServiceActor::new(tx_s_out);
        let tx_s_in = s_actor.get_sender();

        let (tx_bm_out, rx_bm_out) = mpsc::channel(BUFFER_MAX);
        let (tx_bms_out, rx_bms_out) = mpsc::channel(BUFFER_MAX);
        let bm_actor = BotManagerActor::new(tx_bm_out, tx_bms_out);
        let tx_bm_in = bm_actor.get_sender();
        let tx_bms_in = bm_actor.get_service_sender();

        let now = Instant::now();
        let map_hb_times = Arc::new(RwLock::new(HashMap::from([
            (ActorType::MatchMaking, now),
            (ActorType::Game, now),
            (ActorType::Service, now),
            (ActorType::BotManager, now),
        ])));

        Self {
            state: ActorState {
                tx_mm_in,
                tx_g_in,
                tx_s_in,
                tx_bm_in,
                tx_bms_in,
                map_hb_times,
            },
            rx_mm_out,
            mm_actor,
            rx_g_out,
            g_actor,
            rx_s_out,
            s_actor: Some(s_actor),
            rx_bm_out,
            rx_bms_out,
            bm_actor,
        }
    }

    #[cfg(test)]
    pub fn new_no_service(
        tx_s_in: Sender<s::MessageIn>,
        rx_s_out: Receiver<s::MessageOut>,
    ) -> Self {
        let (tx_mm_out, rx_mm_out) = mpsc::channel(BUFFER_MAX);
        let mm_actor = mm::MatchMakingActor::new(tx_mm_out, None);
        let tx_mm_in = mm_actor.get_sender();

        let (tx_g_out, rx_g_out) = mpsc::channel(BUFFER_MAX);
        let g_actor = GameActor::new(tx_g_out);
        let tx_g_in = g_actor.get_sender();

        let (tx_bm_out, rx_bm_out) = mpsc::channel(BUFFER_MAX);
        let (tx_bms_out, rx_bms_out) = mpsc::channel(BUFFER_MAX);
        let bm_actor = BotManagerActor::new(tx_bm_out, tx_bms_out);
        let tx_bm_in = bm_actor.get_sender();
        let tx_bms_in = bm_actor.get_service_sender();

        let map_hb_times = Arc::new(RwLock::new(HashMap::new()));

        Self {
            state: ActorState {
                tx_mm_in,
                tx_g_in,
                tx_s_in,
                tx_bm_in,
                tx_bms_in,
                map_hb_times,
            },
            rx_mm_out,
            mm_actor,
            rx_g_out,
            g_actor,
            rx_s_out,
            s_actor: None,
            rx_bm_out,
            rx_bms_out,
            bm_actor,
        }
    }

    pub async fn run_all(self) {
        log::debug!("Controller starting all actors...");
        let ActorController {
            state,
            mut rx_mm_out,
            mm_actor,
            mut rx_g_out,
            g_actor,
            mut rx_s_out,
            s_actor,
            mut rx_bm_out,
            mut rx_bms_out,
            bm_actor,
        } = self;

        if let Some(s_actor) = s_actor {
            s_actor.start();
        } else {
            log::warn!("Service actor not started. Assuming this is a test env");
        }

        mm_actor.start();
        g_actor.start();
        bm_actor.start();

        log::debug!("Actor Controller started");
        let mut tasks = JoinSet::new();

        let state_1 = state.clone();
        tasks.spawn(async move {
            let state = state_1;
            loop {
                sleep(HB_RECV_WAIT_LIMIT).await;
                let rmap = state.map_hb_times.read().await;
                rmap.iter().for_each(|(actor, time_of_hb)| {
                    let elapsed = time_of_hb.elapsed();
                    if elapsed > HB_RECV_WAIT_LIMIT {
                        log::warn!(
                            "Have not received HB from {:?} actor since {}s ago",
                            actor,
                            elapsed.as_secs()
                        );
                    }
                });
            }
        });

        let state_2 = state.clone();
        tasks.spawn(async move {
            let state = state_2;
            loop {
                tokio::select! {
                    Some(msg) = rx_s_out.recv() => {
                        log::debug!("RECV {msg:?} from S actor");
                        if let Err(e) = Self::handle_msg_s_out(&state, msg).await {
                            log::error!("{e}");
                        }
                    }
                    Some(msg) = rx_bms_out.recv() => {
                        log::debug!("RECV {msg:?} from BM actor");
                        if let Err(e) = Self::handle_msg_s_out(&state, msg).await {
                            log::error!("{e}");
                        }
                    }
                    Some(msg) = rx_mm_out.recv() => {
                        log::debug!("RECV {msg:?} from MM actor");
                        if let Err(e) = Self::handle_msg_mm_out(&state, msg).await {
                            log::error!("{e}");
                        }
                    },
                    Some(msg) = rx_g_out.recv() => {
                        log::debug!("RECV {msg:?} from G actor");
                        if let Err(e) = Self::handle_msg_g_out(&state, msg).await {
                            log::error!("{e}");
                        }
                    },
                    Some(msg) = rx_bm_out.recv() => {
                        log::debug!("RECV {msg:?} from BM actor");
                        if let Err(e) = Self::handle_msg_bm_out(&state, msg).await {
                            log::error!("{e}");
                        }
                    }
                }
            }
        });

        if tasks.join_next().await.is_some() {
            log::error!("Terminating ActorController");
            tasks.abort_all();
        }
    }

    async fn handle_msg_s_out(
        state: &ActorState,
        msg: s::MessageOut,
    ) -> Result<(), ActorSendError> {
        let s::MessageOut { user, inner: msg } = msg;
        match msg {
            s::MessageOutInner::SearchGame => {
                state.tx_mm_in.send(mm::MessageIn::Search { user }).await?;
            }
            s::MessageOutInner::Move { col } => {
                state.tx_g_in.send(g::MessageIn::Move { user, col }).await?;
            }
            s::MessageOutInner::UserLeft => {
                state
                    .tx_g_in
                    .send(g::MessageIn::UserLeft { user: user.clone() })
                    .await?;
                state
                    .tx_mm_in
                    .send(mm::MessageIn::CancelSearch { user })
                    .await?;
            }
            s::MessageOutInner::QueryGetState { respond_to } => {
                let (tx_mm, rx_mm) = oneshot::channel();
                let (tx_g, rx_g) = oneshot::channel();
                state
                    .tx_mm_in
                    .send(mm::MessageIn::QueryGetState { respond_to: tx_mm })
                    .await?;
                state
                    .tx_g_in
                    .send(g::MessageIn::QueryGetState { respond_to: tx_g })
                    .await?;
                let Ok(mm_state) = rx_mm.await else {
                    log::error!("Could not get state from MM actor");
                    return Ok(());
                };
                let Ok(g_state) = rx_g.await else {
                    log::error!("Could not get state from MM actor");
                    return Ok(());
                };
                respond_to
                    .send((mm_state, g_state))
                    .map_err(|_| ActorSendError::Oneshot)?;
            }
            s::MessageOutInner::SpawnSeveralBots { number } => {
                state
                    .tx_bm_in
                    .send(bm::MessageIn::SpawnSeveral { number })
                    .await?;
            }
            s::MessageOutInner::HeartBeat => {
                state
                    .map_hb_times
                    .write()
                    .await
                    .insert(ActorType::Service, Instant::now());
            }
        }
        Ok(())
    }

    async fn handle_msg_mm_out(
        state: &ActorState,
        msg: mm::MessageOut,
    ) -> Result<(), ActorSendError> {
        match msg {
            mm::MessageOut::UsersFound { users } => {
                state.tx_g_in.send(g::MessageIn::NewGame { users }).await?;
            }
            mm::MessageOut::LongWait { user: _ } => {
                state.tx_bm_in.send(bm::MessageIn::QueueOne).await?;
            }
            mm::MessageOut::HeartBeat => {
                state
                    .map_hb_times
                    .write()
                    .await
                    .insert(ActorType::MatchMaking, Instant::now());
            }
        }
        Ok(())
    }

    async fn handle_msg_g_out(
        state: &ActorState,
        msg: g::MessageOut,
    ) -> Result<(), ActorSendError> {
        match msg {
            g::MessageOut::NewGame {
                game_id,
                users,
                first_turn,
            } => {
                let (user0, user1) = users;
                Self::send_to_service(
                    &state.tx_s_in,
                    &state.tx_bms_in,
                    s::MessageIn {
                        user: user0.clone(),
                        inner: s::MessageInInner::NewGame {
                            game_id,
                            rival: user1.clone(),
                            first_turn: first_turn == user0,
                        },
                    },
                )
                .await?;
                Self::send_to_service(
                    &state.tx_s_in,
                    &state.tx_bms_in,
                    s::MessageIn {
                        user: user1.clone(),
                        inner: s::MessageInInner::NewGame {
                            game_id,
                            rival: user0.clone(),
                            first_turn: first_turn == user1,
                        },
                    },
                )
                .await?;
            }
            g::MessageOut::UserAlreadyInGame {
                reject_users,
                rematchmake_users,
            } => {
                for user in reject_users {
                    log::warn!("User {user} tried to matchmake but is already in a game.");
                }
                for user in rematchmake_users {
                    state.tx_mm_in.send(mm::MessageIn::Search { user }).await?;
                }
            }
            g::MessageOut::UserNotInGame { user } => {
                log::warn!("User {user} tried to make a MOVE event, but he is not in a game.");
            }
            g::MessageOut::MoveValid {
                player,
                rival,
                valid,
                col,
            } => {
                if valid {
                    Self::send_to_service(
                        &state.tx_s_in,
                        &state.tx_bms_in,
                        s::MessageIn {
                            user: player,
                            inner: s::MessageInInner::MoveValid { valid: true },
                        },
                    )
                    .await?;
                    Self::send_to_service(
                        &state.tx_s_in,
                        &state.tx_bms_in,
                        s::MessageIn {
                            user: rival,
                            inner: s::MessageInInner::RivalMove { col },
                        },
                    )
                    .await?;
                } else {
                    Self::send_to_service(
                        &state.tx_s_in,
                        &state.tx_bms_in,
                        s::MessageIn {
                            user: player,
                            inner: s::MessageInInner::MoveValid { valid: false },
                        },
                    )
                    .await?;
                }
            }
            g::MessageOut::GameOverWinner { winner, loser } => {
                Self::send_to_service(
                    &state.tx_s_in,
                    &state.tx_bms_in,
                    s::MessageIn {
                        user: winner,
                        inner: s::MessageInInner::GameOver { won: Some(true) },
                    },
                )
                .await?;
                Self::send_to_service(
                    &state.tx_s_in,
                    &state.tx_bms_in,
                    s::MessageIn {
                        user: loser,
                        inner: s::MessageInInner::GameOver { won: Some(false) },
                    },
                )
                .await?;
            }
            g::MessageOut::GameOverDraw { users } => {
                Self::send_to_service(
                    &state.tx_s_in,
                    &state.tx_bms_in,
                    s::MessageIn {
                        user: users.0,
                        inner: s::MessageInInner::GameOver { won: None },
                    },
                )
                .await?;
                Self::send_to_service(
                    &state.tx_s_in,
                    &state.tx_bms_in,
                    s::MessageIn {
                        user: users.1,
                        inner: s::MessageInInner::GameOver { won: None },
                    },
                )
                .await?;
            }
            g::MessageOut::AbortGame { user } => {
                Self::send_to_service(
                    &state.tx_s_in,
                    &state.tx_bms_in,
                    s::MessageIn {
                        user,
                        inner: s::MessageInInner::RivalLeft,
                    },
                )
                .await?;
            }
            g::MessageOut::HeartBeat => {
                state
                    .map_hb_times
                    .write()
                    .await
                    .insert(ActorType::Game, Instant::now());
            }
        }
        Ok(())
    }

    async fn handle_msg_bm_out(
        state: &ActorState,
        msg: bm::MessageOut,
    ) -> Result<(), ActorSendError> {
        match msg {
            bm::MessageOut::HeartBeat => {
                state
                    .map_hb_times
                    .write()
                    .await
                    .insert(ActorType::BotManager, Instant::now());
            }
        }
        Ok(())
    }

    async fn send_to_service(
        tx_s_in: &Sender<s::MessageIn>,
        tx_bms_in: &Sender<s::MessageIn>,
        msg: s::MessageIn,
    ) -> Result<(), ActorSendError> {
        tx_s_in.send(msg.clone()).await?;
        tx_bms_in.send(msg).await?;
        Ok(())
    }
}
