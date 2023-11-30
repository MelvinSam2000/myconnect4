use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
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
    bot_id: String,
    tx_in: Sender<MessageIn>,
    #[allow(dead_code)]
    tx_out: Sender<MessageOut>,
    rx_in: Receiver<MessageIn>,
    tx_s_in: Sender<service::MessageInInner>,
    tx_s_out: Sender<service::MessageOut>,
    rx_s_in: Receiver<service::MessageInInner>,
    state: Arc<Mutex<BotState>>,
    perma_play: Arc<AtomicBool>,
}

pub enum BotState {
    Idle,
    Searching,
    Playing { game: Connect4Game },
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
        Self {
            bot_id: bot_id,
            tx_in,
            tx_out,
            rx_in,
            tx_s_in,
            tx_s_out,
            rx_s_in,
            state: Arc::new(Mutex::new(BotState::Idle)),
            perma_play: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn get_id(&self) -> String {
        self.bot_id.clone()
    }

    pub fn get_sender(&self) -> Sender<MessageIn> {
        self.tx_in.clone()
    }

    pub fn get_service_sender(&self) -> Sender<service::MessageInInner> {
        self.tx_s_in.clone()
    }

    pub fn start(mut self) {
        // handle messages from bot manager
        let botstate_1 = self.state.clone();
        let botstate_2 = self.state.clone();
        let bot_id_1 = self.bot_id.clone();
        let bot_id_2 = self.bot_id.clone();

        let tx_s_out = self.tx_s_out.clone();
        let perma_play = self.perma_play.clone();

        tokio::spawn(async move {
            let botstate = botstate_1;
            let bot_id = bot_id_1;
            log::debug!("Started bot actor {bot_id}");
            while let Some(msg) = self.rx_in.recv().await {
                log::debug!("{bot_id} RECV {msg:?}");
                match msg {
                    MessageIn::SearchForGame => {
                        let mut botstate = botstate.lock().await;
                        let BotState::Idle = *botstate else {
                            return;
                        };
                        *botstate = BotState::Searching;
                        self.tx_s_out
                            .send(service::MessageOut {
                                user: self.bot_id.clone(),
                                inner: service::MessageOutInner::SearchGame,
                            })
                            .await
                            .unwrap();
                    }
                    MessageIn::QueryIdle { respond_to } => {
                        let botstate = botstate.lock().await;
                        let idle = if let BotState::Idle = *botstate {
                            true
                        } else {
                            false
                        };
                        respond_to.send(idle).unwrap();
                    }
                    MessageIn::PermaPlay(mode) => {
                        self.perma_play
                            .store(mode, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
        });

        // handle messages from game update
        tokio::spawn(async move {
            let botstate = botstate_2;
            let bot_id = bot_id_2;
            while let Some(msg) = self.rx_s_in.recv().await {
                log::debug!("{bot_id} RECV {msg:?}");
                match msg {
                    service::MessageInInner::NewGame {
                        game_id,
                        rival,
                        first_turn,
                    } => {
                        let mut botstate = botstate.lock().await;
                        let mut game =
                            Connect4Game::new_custom(game_id, bot_id.clone(), rival, first_turn);
                        if first_turn {
                            Self::play(&bot_id, &mut game, &tx_s_out).await;
                        }
                        *botstate = BotState::Playing { game };
                    }
                    service::MessageInInner::MoveValid { valid } => {
                        if valid == false {
                            log::error!("{bot_id} made an invalid move?");
                        }
                    }
                    service::MessageInInner::RivalMove { col } => {
                        let mut botstate = botstate.lock().await;
                        let BotState::Playing { game } = &mut *botstate else {
                            log::warn!(
                                "Received a rivalmove even though bot is not in playing state..."
                            );
                            continue;
                        };
                        if !game.play_no_user(col) {
                            log::error!("Rival made an invalid move");
                            continue;
                        }
                        if game.is_gameover().is_some() {
                            continue;
                        }
                        Self::play(&bot_id, game, &tx_s_out).await;
                    }
                    service::MessageInInner::GameOver { won } => {
                        let result = match won {
                            None => "DRAW",
                            Some(false) => "LOST",
                            Some(true) => "VICTORY",
                        };
                        log::debug!("Game over for {bot_id}: {result}");
                        let mut botstate = botstate.lock().await;
                        *botstate = BotState::Idle;
                        if perma_play.load(std::sync::atomic::Ordering::SeqCst) {
                            self.tx_in.send(MessageIn::SearchForGame).await.unwrap();
                        }
                    }
                    service::MessageInInner::RivalLeft => {
                        let mut botstate = botstate.lock().await;
                        let BotState::Playing { game: _ } = &*botstate else {
                            log::warn!(
                                "Received a rivalleft even though bot is not in playing state..."
                            );
                            continue;
                        };
                        *botstate = BotState::Idle;
                        if perma_play.load(std::sync::atomic::Ordering::SeqCst) {
                            self.tx_in.send(MessageIn::SearchForGame).await.unwrap();
                        }
                    }
                }
            }
        });
    }

    async fn play(bot_id: &str, game: &mut Connect4Game, tx_s_out: &Sender<service::MessageOut>) {
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
            return;
        }
        tx_s_out
            .send(service::MessageOut {
                user: bot_id.to_string(),
                inner: service::MessageOutInner::Move { col },
            })
            .await
            .unwrap();
    }
}
