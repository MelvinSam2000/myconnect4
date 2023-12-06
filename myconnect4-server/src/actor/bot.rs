use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio::time::sleep;

use super::service;
use super::BUFFER_MAX;
use crate::game::Connect4Game;
use crate::game::COLS;

#[derive(Debug)]
pub enum MessageIn {
    SearchForGame,
    PermaPlay(bool),
    QueryIdle { respond_to: oneshot::Sender<bool> },
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageOut {
    Nothing,
}

pub struct BotActor {
    state: ActorState,
    rx_in: Receiver<MessageIn>,
    rx_s_in: Receiver<service::MessageInInner>,
}

#[derive(Clone)]
struct ActorState {
    tx_in: Sender<MessageIn>,
    #[allow(dead_code)]
    tx_out: Sender<MessageOut>,
    tx_s_in: Sender<service::MessageInInner>,
    tx_s_out: Sender<service::MessageOut>,
    bot_id: String,
    bot_state: Arc<Mutex<BotState>>,
    perma_play: Arc<AtomicBool>,
}

pub enum BotState {
    Idle,
    Searching,
    Playing { game: Connect4Game },
}

#[derive(Debug, Error)]
enum ActorSendError {
    #[error("Error sending msg: {0}")]
    MessageIn(#[from] SendError<MessageIn>),
    #[error("Error sending msg: {0}")]
    MessageOut(#[from] SendError<MessageOut>),
    #[error("Error sending msg: {0}")]
    ServiceMessageIn(#[from] SendError<service::MessageIn>),
    #[error("Error sending msg: {0}")]
    ServiceMessageOut(#[from] SendError<service::MessageOut>),
    #[error("Error sending oneshot")]
    Oneshot,
}

impl std::fmt::Debug for BotState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Searching => write!(f, "Searching"),
            Self::Playing { game } => write!(f, "Playing game:[{}]", game.game_id),
        }
    }
}

impl BotActor {
    pub fn new(tx_out: Sender<MessageOut>, tx_s_out: Sender<service::MessageOut>) -> Self {
        let (tx_in, rx_in) = mpsc::channel(BUFFER_MAX);
        let (tx_s_in, rx_s_in) = mpsc::channel(BUFFER_MAX);
        let id = rand::random::<u64>();
        let bot_id = format!("bot-{id}");
        let bot_state = Arc::new(Mutex::new(BotState::Idle));
        let perma_play = Arc::new(AtomicBool::new(false));

        Self {
            state: ActorState {
                tx_in,
                tx_out,
                tx_s_in,
                tx_s_out,
                bot_id,
                bot_state,
                perma_play,
            },
            rx_in,
            rx_s_in,
        }
    }

    pub fn get_id(&self) -> String {
        self.state.bot_id.clone()
    }

    pub fn get_sender(&self) -> Sender<MessageIn> {
        self.state.tx_in.clone()
    }

    pub fn get_service_sender(&self) -> Sender<service::MessageInInner> {
        self.state.tx_s_in.clone()
    }

    pub fn start(self) {
        let BotActor {
            state,
            mut rx_in,
            mut rx_s_in,
        } = self;

        let mut tasks = JoinSet::new();

        // handle messages from bot manager
        let state_1 = state.clone();
        tasks.spawn(async move {
            let state = state_1;
            log::debug!("Started bot actor {}", &state.bot_id);
            while let Some(msg) = rx_in.recv().await {
                log::debug!("RECV {msg:?} from {}", &state.bot_id);
                if let Err(e) = Self::handle_msg_in(&state, msg).await {
                    log::error!("{e}");
                }
            }
        });

        // handle messages from game update
        let state_2 = state.clone();
        tasks.spawn(async move {
            let state = state_2;
            while let Some(msg) = rx_s_in.recv().await {
                log::debug!("RECV {msg:?} from {}", &state.bot_id);
                if let Err(e) = Self::handle_service_msg_in(&state, msg).await {
                    log::error!("{e}");
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

    async fn play(
        bot_id: &str,
        game: &mut Connect4Game,
        tx_s_out: &Sender<service::MessageOut>,
    ) -> Result<(), ActorSendError> {
        sleep(Duration::from_millis(300)).await;
        let mut valid = false;
        let mut col = 0;
        for _ in 0..100 {
            col = rand::random::<u8>() % COLS as u8;
            if game.play_no_user(col) {
                valid = true;
                break;
            }
        }
        if !valid {
            log::debug!("Bot unable to make another move. Waiting on game over event.");
            return Ok(());
        }
        tx_s_out
            .send(service::MessageOut {
                user: bot_id.to_string(),
                inner: service::MessageOutInner::Move { col },
            })
            .await?;
        Ok(())
    }

    async fn handle_msg_in(state: &ActorState, msg: MessageIn) -> Result<(), ActorSendError> {
        match msg {
            MessageIn::SearchForGame => {
                let mut botstate = state.bot_state.lock().await;
                if !matches!(*botstate, BotState::Idle) {
                    return Ok(());
                }
                *botstate = BotState::Searching;
                drop(botstate);
                state
                    .tx_s_out
                    .send(service::MessageOut {
                        user: state.bot_id.clone(),
                        inner: service::MessageOutInner::SearchGame,
                    })
                    .await?;
            }
            MessageIn::QueryIdle { respond_to } => {
                let botstate = state.bot_state.lock().await;
                let idle = matches!(*botstate, BotState::Idle);
                drop(botstate);
                respond_to.send(idle).map_err(|_| ActorSendError::Oneshot)?;
            }
            MessageIn::PermaPlay(mode) => {
                state
                    .perma_play
                    .store(mode, std::sync::atomic::Ordering::SeqCst);
            }
        }
        Ok(())
    }

    async fn handle_service_msg_in(
        state: &ActorState,
        msg: service::MessageInInner,
    ) -> Result<(), ActorSendError> {
        match msg {
            service::MessageInInner::NewGame {
                game_id,
                rival,
                first_turn,
            } => {
                let BotState::Searching = *state.bot_state.lock().await else {
                    return Ok(());
                };
                let mut game =
                    Connect4Game::new_custom(game_id, state.bot_id.clone(), rival, first_turn);
                if first_turn {
                    Self::play(&state.bot_id, &mut game, &state.tx_s_out).await?;
                }
                *state.bot_state.lock().await = BotState::Playing { game };
            }
            service::MessageInInner::MoveValid { valid } => {
                if !valid {
                    log::error!("{} made an invalid move?", &state.bot_id);
                }
            }
            service::MessageInInner::RivalMove { col } => {
                let mut botstate = state.bot_state.lock().await;
                let BotState::Playing { game } = &mut *botstate else {
                    log::warn!("Received a rivalmove even though bot is not in playing state...");
                    return Ok(());
                };
                if !game.play_no_user(col) {
                    log::error!("Rival made an invalid move");
                    return Ok(());
                }
                if game.is_gameover().is_some() {
                    return Ok(());
                }
                Self::play(&state.bot_id, game, &state.tx_s_out).await?;
            }
            service::MessageInInner::GameOver { won } => {
                let result = match won {
                    None => "DRAW",
                    Some(false) => "LOST",
                    Some(true) => "VICTORY",
                };
                log::debug!("Game over for {}: {result}", &state.bot_id);
                let mut botstate = state.bot_state.lock().await;
                *botstate = BotState::Idle;
                if state.perma_play.load(std::sync::atomic::Ordering::SeqCst) {
                    state.tx_in.send(MessageIn::SearchForGame).await?;
                }
            }
            service::MessageInInner::RivalLeft => {
                let mut botstate = state.bot_state.lock().await;
                let BotState::Playing { game: _ } = &*botstate else {
                    log::warn!("Received a rivalleft even though bot is not in playing state...");
                    return Ok(());
                };
                *botstate = BotState::Idle;
                if state.perma_play.load(std::sync::atomic::Ordering::SeqCst) {
                    state.tx_in.send(MessageIn::SearchForGame).await?;
                }
            }
        }
        Ok(())
    }
}
