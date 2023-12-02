use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::sleep;

use super::botmanager as bm;
use super::game as g;
use super::matchmaking as mm;
use super::matchmaking::MatchMakingActor;
use super::service as s;
use super::service::ServiceActor;
use super::BUFFER_MAX;
use crate::actor::botmanager::BotManagerActor;
use crate::actor::game::GameActor;

pub struct ActorController {
    tx_mm_in: Sender<mm::MessageIn>,
    rx_mm_out: Receiver<mm::MessageOut>,
    mm_actor: MatchMakingActor,

    tx_g_in: Sender<g::MessageIn>,
    rx_g_out: Receiver<g::MessageOut>,
    g_actor: GameActor,

    tx_s_in: Sender<s::MessageIn>,
    rx_s_out: Receiver<s::MessageOut>,
    s_actor: Option<ServiceActor>,

    tx_bm_in: Sender<bm::MessageIn>,
    rx_bm_out: Receiver<bm::MessageOut>,
    tx_bms_in: Sender<s::MessageIn>,
    rx_bms_out: Receiver<s::MessageOut>,
    bm_actor: BotManagerActor,
}

#[derive(Clone)]
struct Senders {
    tx_mm_in: Sender<mm::MessageIn>,
    tx_g_in: Sender<g::MessageIn>,
    tx_s_in: Sender<s::MessageIn>,
    tx_bm_in: Sender<bm::MessageIn>,
    tx_bms_in: Sender<s::MessageIn>,
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

        Self {
            tx_mm_in,
            rx_mm_out,
            mm_actor,
            tx_g_in,
            rx_g_out,
            g_actor,
            tx_s_in,
            rx_s_out,
            s_actor: Some(s_actor),
            tx_bm_in,
            rx_bm_out,
            tx_bms_in,
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

        Self {
            tx_mm_in,
            rx_mm_out,
            mm_actor,
            tx_g_in,
            rx_g_out,
            g_actor,
            tx_s_in,
            rx_s_out,
            s_actor: None,
            tx_bm_in,
            rx_bm_out,
            tx_bms_in,
            rx_bms_out,
            bm_actor,
        }
    }

    pub async fn run_all(self) {
        log::debug!("Controller starting all actors...");
        let ActorController {
            tx_mm_in,
            mut rx_mm_out,
            mm_actor,
            tx_g_in,
            mut rx_g_out,
            g_actor,
            tx_s_in,
            mut rx_s_out,
            s_actor,
            tx_bm_in,
            mut rx_bm_out,
            tx_bms_in,
            mut rx_bms_out,
            bm_actor,
        } = self;

        let senders = Senders {
            tx_mm_in,
            tx_g_in,
            tx_s_in,
            tx_bm_in,
            tx_bms_in,
        };

        if let Some(s_actor) = s_actor {
            s_actor.start();
        } else {
            log::warn!("service actor not started. Assuming this is a test env");
        }

        mm_actor.start();
        g_actor.start();
        bm_actor.start();

        let senders_1 = senders.clone();
        tokio::spawn(async move {
            let senders = senders_1;
            loop {
                sleep(Duration::from_millis(10000)).await;
                let mut joinset = JoinSet::new();
                let tx_mm_in = senders.tx_mm_in.clone();
                joinset.spawn(Self::hb_actor("MatchMaking", tx_mm_in, |respond_to| {
                    mm::MessageIn::HeartBeat { respond_to }
                }));
                let tx_g_in = senders.tx_g_in.clone();
                joinset.spawn(Self::hb_actor("Game", tx_g_in, |respond_to| {
                    g::MessageIn::HeartBeat { respond_to }
                }));
                /* TODO: Support HB for service actor
                let tx_s_in = senders.tx_s_in.clone();
                joinset.spawn(Self::hb_actor("Game", tx_s_in, |respond_to| {
                    s::MessageIn::HeartBeat { respond_to }
                }));
                */
                let tx_bm_in = senders.tx_bm_in.clone();
                joinset.spawn(Self::hb_actor("BotManager", tx_bm_in, |respond_to| {
                    bm::MessageIn::HeartBeat { respond_to }
                }));

                while joinset.join_next().await.is_some() {}
            }
        });

        log::debug!("Controller actor started.");
        let senders_2 = senders.clone();
        let _ = tokio::spawn(async move {
            let senders = senders_2;
            loop {
                tokio::select! {
                    Some(msg) = rx_s_out.recv() => {
                        log::debug!("RECV {msg:?} from S actor");
                        Self::handle_msg_s_out(&senders, msg).await;
                    }
                    Some(msg) = rx_bms_out.recv() => {
                        log::debug!("RECV {msg:?} from BM actor");
                        Self::handle_msg_s_out(&senders, msg).await;
                    }
                    Some(msg) = rx_mm_out.recv() => {
                        log::debug!("RECV {msg:?} from MM actor");
                        Self::handle_msg_mm_out(&senders, msg).await;
                    },
                    Some(msg) = rx_g_out.recv() => {
                        log::debug!("RECV {msg:?} from G actor");
                        Self::handle_msg_g_out(&senders, msg).await;
                    },
                    Some(msg) = rx_bm_out.recv() => {
                        log::debug!("RECV {msg:?} from BM actor");
                        Self::handle_msg_bm_out(msg).await;
                    }
                }
            }
        })
        .await;
    }

    async fn hb_actor<T>(
        actor_label: &str,
        tx_actor: Sender<T>,
        hb: impl FnOnce(oneshot::Sender<()>) -> T,
    ) {
        let task = async {
            let (tx, rx) = oneshot::channel();
            tx_actor
                //.send(mm::MessageIn::HeartBeat { respond_to: tx })
                .send(hb(tx))
                .await
                .unwrap();
            rx.await
        };
        tokio::select! {
            res = task => {
                match res {
                    Ok(_) => log::debug!("HB from {actor_label} received"),
                    Err(_) => log::error!("Did not receive HB from {actor_label} actor")
                }
            }
            _ = sleep(Duration::from_millis(10000)) => {
                log::error!("Did not receive HB from {actor_label} actor");
            }
        }
    }

    async fn handle_msg_s_out(senders: &Senders, msg: s::MessageOut) {
        let s::MessageOut { user, inner: msg } = msg;
        match msg {
            s::MessageOutInner::SearchGame => {
                senders
                    .tx_mm_in
                    .send(mm::MessageIn::Search { user })
                    .await
                    .unwrap();
            }
            s::MessageOutInner::Move { col } => {
                senders
                    .tx_g_in
                    .send(g::MessageIn::Move { user, col })
                    .await
                    .unwrap();
            }
            s::MessageOutInner::UserLeft => {
                senders
                    .tx_g_in
                    .send(g::MessageIn::UserLeft { user: user.clone() })
                    .await
                    .unwrap();
                senders
                    .tx_mm_in
                    .send(mm::MessageIn::CancelSearch { user })
                    .await
                    .unwrap();
            }
            s::MessageOutInner::QueryGetState { respond_to } => {
                let (tx_mm, rx_mm) = oneshot::channel();
                let (tx_g, rx_g) = oneshot::channel();
                senders
                    .tx_mm_in
                    .send(mm::MessageIn::QueryGetState { respond_to: tx_mm })
                    .await
                    .unwrap();
                senders
                    .tx_g_in
                    .send(g::MessageIn::QueryGetState { respond_to: tx_g })
                    .await
                    .unwrap();
                let mm_state = rx_mm.await.unwrap();
                let g_state = rx_g.await.unwrap();
                respond_to.send((mm_state, g_state)).unwrap();
            }
            s::MessageOutInner::SpawnSeveralBots { number } => senders
                .tx_bm_in
                .send(bm::MessageIn::SpawnSeveral { number })
                .await
                .unwrap(),
        }
    }

    async fn handle_msg_mm_out(senders: &Senders, msg: mm::MessageOut) {
        match msg {
            mm::MessageOut::UsersFound { users } => {
                senders
                    .tx_g_in
                    .send(g::MessageIn::NewGame { users })
                    .await
                    .unwrap();
            }
            mm::MessageOut::LongWait { user: _ } => senders
                .tx_bm_in
                .send(bm::MessageIn::QueueOne)
                .await
                .unwrap(),
        }
    }

    async fn handle_msg_g_out(senders: &Senders, msg: g::MessageOut) {
        match msg {
            g::MessageOut::NewGame {
                game_id,
                users,
                first_turn,
            } => {
                let (user0, user1) = users;
                Self::send_to_service(
                    &senders.tx_s_in,
                    &senders.tx_bms_in,
                    s::MessageIn {
                        user: user0.clone(),
                        inner: s::MessageInInner::NewGame {
                            game_id,
                            rival: user1.clone(),
                            first_turn: first_turn == user0,
                        },
                    },
                )
                .await;
                Self::send_to_service(
                    &senders.tx_s_in,
                    &senders.tx_bms_in,
                    s::MessageIn {
                        user: user1.clone(),
                        inner: s::MessageInInner::NewGame {
                            game_id,
                            rival: user0.clone(),
                            first_turn: first_turn == user1,
                        },
                    },
                )
                .await;
            }
            g::MessageOut::UserAlreadyInGame {
                reject_users,
                rematchmake_users,
            } => {
                for user in reject_users {
                    log::warn!("User {user} tried to matchmake but is already in a game.");
                }
                for user in rematchmake_users {
                    senders
                        .tx_mm_in
                        .send(mm::MessageIn::Search { user })
                        .await
                        .unwrap();
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
                        &senders.tx_s_in,
                        &senders.tx_bms_in,
                        s::MessageIn {
                            user: player,
                            inner: s::MessageInInner::MoveValid { valid: true },
                        },
                    )
                    .await;
                    Self::send_to_service(
                        &senders.tx_s_in,
                        &senders.tx_bms_in,
                        s::MessageIn {
                            user: rival,
                            inner: s::MessageInInner::RivalMove { col },
                        },
                    )
                    .await;
                } else {
                    Self::send_to_service(
                        &senders.tx_s_in,
                        &senders.tx_bms_in,
                        s::MessageIn {
                            user: player,
                            inner: s::MessageInInner::MoveValid { valid: false },
                        },
                    )
                    .await;
                }
            }
            g::MessageOut::GameOverWinner { winner, loser } => {
                Self::send_to_service(
                    &senders.tx_s_in,
                    &senders.tx_bms_in,
                    s::MessageIn {
                        user: winner,
                        inner: s::MessageInInner::GameOver { won: Some(true) },
                    },
                )
                .await;
                Self::send_to_service(
                    &senders.tx_s_in,
                    &senders.tx_bms_in,
                    s::MessageIn {
                        user: loser,
                        inner: s::MessageInInner::GameOver { won: Some(false) },
                    },
                )
                .await;
            }
            g::MessageOut::GameOverDraw { users } => {
                Self::send_to_service(
                    &senders.tx_s_in,
                    &senders.tx_bms_in,
                    s::MessageIn {
                        user: users.0,
                        inner: s::MessageInInner::GameOver { won: None },
                    },
                )
                .await;
                Self::send_to_service(
                    &senders.tx_s_in,
                    &senders.tx_bms_in,
                    s::MessageIn {
                        user: users.1,
                        inner: s::MessageInInner::GameOver { won: None },
                    },
                )
                .await;
            }
            g::MessageOut::AbortGame { user } => {
                Self::send_to_service(
                    &senders.tx_s_in,
                    &senders.tx_bms_in,
                    s::MessageIn {
                        user,
                        inner: s::MessageInInner::RivalLeft,
                    },
                )
                .await;
            }
        }
    }

    async fn handle_msg_bm_out(msg: bm::MessageOut) {
        match msg {
            bm::MessageOut::Nothing => log::debug!("Ignoring."),
        }
    }

    async fn send_to_service(
        tx_s_in: &Sender<s::MessageIn>,
        tx_bms_in: &Sender<s::MessageIn>,
        msg: s::MessageIn,
    ) {
        tx_s_in.send(msg.clone()).await.unwrap();
        tx_bms_in.send(msg).await.unwrap();
    }
}
