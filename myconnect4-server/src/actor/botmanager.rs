use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

use either::Either;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio::time::sleep;

use super::bot;
use super::service;
use super::BUFFER_MAX;
use crate::actor::bot::BotActor;
use crate::actor::HB_SEND_DUR;
use crate::minimax;

#[derive(Debug)]
pub enum MessageIn {
    HeartBeat { respond_to: oneshot::Sender<()> },
    QueueOne,
    SpawnSeveral { number: usize },
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageOut {
    HeartBeat,
}

pub struct BotManagerActor {
    state: ActorState,
    rx_in: Receiver<MessageIn>,
    rx_s_in: Receiver<service::MessageIn>,
    rx_b_out: Receiver<bot::MessageOut>,
}

#[derive(Clone)]
struct ActorState {
    tx_in: Sender<MessageIn>,
    #[allow(dead_code)]
    tx_out: Sender<MessageOut>,
    tx_s_in: Sender<service::MessageIn>,
    tx_s_out: Sender<service::MessageOut>,
    tx_b_out: Sender<bot::MessageOut>,
    map_bots:
        Arc<RwLock<HashMap<String, (Sender<bot::MessageIn>, Sender<service::MessageInInner>)>>>,
}

#[derive(Debug, Error)]
enum ActorChannelError {
    #[error("Error sending msg: {0}")]
    MessageIn(#[from] SendError<MessageIn>),
    #[error("Error sending msg: {0}")]
    MessageOut(#[from] SendError<MessageOut>),
    #[error("Error sending msg: {0}")]
    ServiceMessageIn(#[from] SendError<service::MessageInInner>),
    #[error("Error sending msg: {0}")]
    ServiceMessageOut(#[from] SendError<service::MessageOut>),
    #[error("Error sending msg: {0}")]
    BotMessageIn(#[from] SendError<bot::MessageIn>),
    #[error("Error sending msg: {0}")]
    BotMessageOut(#[from] SendError<bot::MessageOut>),
    #[error("Error sending oneshot")]
    OneshotSend,
    #[error("Error receiving oneshot")]
    OneshotRecv,
}

impl BotManagerActor {
    pub fn new(tx_out: Sender<MessageOut>, tx_s_out: Sender<service::MessageOut>) -> Self {
        let (tx_in, rx_in) = mpsc::channel(BUFFER_MAX);
        let (tx_s_in, rx_s_in) = mpsc::channel(BUFFER_MAX);
        let (tx_b_out, rx_b_out) = mpsc::channel(BUFFER_MAX);
        let map_bots = Arc::new(RwLock::new(HashMap::new()));
        Self {
            state: ActorState {
                tx_in,
                tx_out,
                tx_s_in,
                tx_s_out,
                tx_b_out,
                map_bots,
            },
            rx_in,
            rx_s_in,
            rx_b_out,
        }
    }

    pub fn get_sender(&self) -> Sender<MessageIn> {
        self.state.tx_in.clone()
    }

    pub fn get_service_sender(&self) -> Sender<service::MessageIn> {
        self.state.tx_s_in.clone()
    }

    async fn send_to_bot(
        map_bots: &Arc<
            RwLock<HashMap<String, (Sender<bot::MessageIn>, Sender<service::MessageInInner>)>>,
        >,
        bot_id: &str,
        msg: Either<bot::MessageIn, service::MessageInInner>,
    ) -> Result<(), ActorChannelError> {
        let map_bots = map_bots.read().await;
        let Some((bsender, ssender)) = map_bots.get(bot_id) else {
            return Ok(());
        };

        match msg {
            Either::Left(msg) => bsender.send(msg).await?,
            Either::Right(msg) => ssender.send(msg).await?,
        }
        Ok(())
    }

    pub fn start(self) {
        let BotManagerActor {
            state,
            mut rx_in,
            mut rx_s_in,
            mut rx_b_out,
        } = self;

        log::debug!("BotManager actor started.");
        let mut tasks = JoinSet::new();
        // main event listening
        let state_1 = state.clone();
        tasks.spawn(async move {
            let state = state_1;
            while let Some(msg) = rx_in.recv().await {
                log::debug!("RECV {msg:?}");
                if let Err(e) = Self::handle_msg_in(&state, msg).await {
                    log::error!("{e}");
                }
            }
        });

        // listening for player events
        let state_2 = state.clone();
        tasks.spawn(async move {
            let state = state_2;
            while let Some(msg) = rx_s_in.recv().await {
                log::debug!("RECV {msg:?}");
                let service::MessageIn { user, inner } = msg;
                if let Err(e) =
                    Self::send_to_bot(&state.map_bots, &user, Either::Right(inner)).await
                {
                    log::error!("Error sending message to bot '{}' due to: {}", &user, e);
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

        // minimax thread
        thread::spawn(move || {
            while let Some(evt) = rx_b_out.blocking_recv() {
                match evt {
                    bot::MessageOut::MinimaxRequest {
                        mut game,
                        respond_to,
                    } => {
                        log::debug!("MINIMAX THREAD RECV: \n{}", game.board_to_str());

                        let bot_move = minimax::best_move(&mut game);

                        if let Err(e) = respond_to.send((game, bot_move)) {
                            log::error!(
                                "Minimax thread cannot respond back to game {}",
                                e.0.game_id
                            );
                        }
                    }
                }
            }
        });

        tokio::spawn(async move {
            if tasks.join_next().await.is_some() {
                log::error!("Terminating ActorController");
                tasks.abort_all();
            }
        });
    }

    async fn handle_msg_in(state: &ActorState, msg: MessageIn) -> Result<(), ActorChannelError> {
        match msg {
            MessageIn::HeartBeat { respond_to } => {
                respond_to
                    .send(())
                    .map_err(|_| ActorChannelError::OneshotSend)?;
            }
            MessageIn::QueueOne => {
                let rmap_bots = state.map_bots.read().await;
                let channels = rmap_bots.values().cloned().collect::<Vec<_>>();
                drop(rmap_bots);

                let mut tasks: JoinSet<Result<(bool, Sender<bot::MessageIn>), ActorChannelError>> =
                    JoinSet::new();
                channels
                    .into_iter()
                    .map(|(sender, _)| {
                        let (tx, rx) = oneshot::channel();
                        async move {
                            sender
                                .send(bot::MessageIn::QueryIdle { respond_to: tx })
                                .await?;
                            let is_idle = rx.await.map_err(|_| ActorChannelError::OneshotRecv)?;
                            Ok((is_idle, sender))
                        }
                    })
                    .for_each(|task| {
                        tasks.spawn(task);
                    });

                while let Some(Ok(res)) = tasks.join_next().await {
                    match res {
                        Ok((is_idle, sender)) => {
                            if is_idle {
                                sender.send(bot::MessageIn::SearchForGame).await?;
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            log::error!("{e}");
                        }
                    }
                }

                let bot_id =
                    Self::spawn_bot(&state.map_bots, &state.tx_b_out, &state.tx_s_out).await;
                Self::send_to_bot(
                    &state.map_bots,
                    &bot_id,
                    Either::Left(bot::MessageIn::SearchForGame),
                )
                .await?;
            }
            MessageIn::SpawnSeveral { number } => {
                let map_bots = state.map_bots.clone();
                let mut tasks: JoinSet<Result<(), ActorChannelError>> = JoinSet::new();
                (0..number)
                    .map(|_| {
                        (
                            map_bots.clone(),
                            state.tx_b_out.clone(),
                            state.tx_s_out.clone(),
                        )
                    })
                    .map(|(map_bots, tx_b_out, tx_s_out)| async move {
                        let bot_id = Self::spawn_bot(&map_bots, &tx_b_out, &tx_s_out).await;
                        Self::send_to_bot(
                            &map_bots,
                            &bot_id,
                            Either::Left(bot::MessageIn::PermaPlay(true)),
                        )
                        .await?;
                        Self::send_to_bot(
                            &map_bots,
                            &bot_id,
                            Either::Left(bot::MessageIn::SearchForGame),
                        )
                        .await?;
                        Ok(())
                    })
                    .for_each(|task| {
                        tasks.spawn(task);
                    });
                log::debug!("Spawning bots...");
                while let Some(res) = tasks.join_next().await {
                    if let Ok(Err(e)) = res {
                        log::error!("Error spawning bot: {e}");
                    }
                }
                log::debug!("Done spawning bots");
            }
        }
        Ok(())
    }

    async fn spawn_bot(
        map_bots: &Arc<
            RwLock<HashMap<String, (Sender<bot::MessageIn>, Sender<service::MessageInInner>)>>,
        >,
        tx_b_out: &Sender<bot::MessageOut>,
        tx_s_out: &Sender<service::MessageOut>,
    ) -> String {
        let tx_out = tx_b_out.clone();
        let tx_s_out = tx_s_out.clone();
        let bot = BotActor::new(tx_out, tx_s_out);

        let bot_id = bot.get_id();
        let bsender = bot.get_sender();
        let ssender = bot.get_service_sender();

        map_bots
            .write()
            .await
            .insert(bot_id.clone(), (bsender, ssender));

        bot.start();

        bot_id
    }
}
