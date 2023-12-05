use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::sleep;

use super::BUFFER_MAX;
use crate::actor::HB_SEND_DUR;
use crate::game::GameOver;
use crate::repo::Connect4Repo;

#[derive(Debug)]
pub enum MessageIn {
    NewGame {
        users: (String, String),
    },
    Move {
        user: String,
        col: u8,
    },
    UserLeft {
        user: String,
    },
    QueryGetState {
        respond_to: oneshot::Sender<StatePayload>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageOut {
    HeartBeat,
    NewGame {
        game_id: u64,
        users: (String, String),
        first_turn: String,
    },
    UserAlreadyInGame {
        reject_users: Vec<String>,
        rematchmake_users: Vec<String>,
    },
    UserNotInGame {
        user: String,
    },
    MoveValid {
        player: String,
        rival: String,
        valid: bool,
        col: u8,
    },
    GameOverWinner {
        winner: String,
        loser: String,
    },
    GameOverDraw {
        users: (String, String),
    },
    AbortGame {
        user: String,
    },
}

pub struct GameActor {
    state: ActorState,
    rx_in: Receiver<MessageIn>,
    repo: Connect4Repo,
}

#[derive(Clone)]
struct ActorState {
    tx_in: Sender<MessageIn>,
    tx_out: Sender<MessageOut>,
}

#[derive(Debug)]
pub struct StatePayload {
    _users_playing: Vec<String>,
    _games: Vec<u64>,
    _num_users_playing: usize,
    _num_games: usize,
    _total_games_played: u128,
}

#[derive(Debug, Error)]
enum ActorSendError {
    #[error("Error sending msg: {0}")]
    MessageIn(#[from] SendError<MessageIn>),
    #[error("Error sending msg: {0}")]
    MessageOut(#[from] SendError<MessageOut>),
}

impl GameActor {
    pub fn new(tx_out: Sender<MessageOut>) -> Self {
        let (tx_in, rx_in) = mpsc::channel(BUFFER_MAX);
        Self {
            state: ActorState { tx_in, tx_out },
            rx_in,
            repo: Connect4Repo::default(),
        }
    }

    pub fn get_sender(&self) -> Sender<MessageIn> {
        self.state.tx_in.clone()
    }

    pub fn start(self) {
        let GameActor {
            state,
            mut rx_in,
            mut repo,
        } = self;

        log::debug!("Game actor started.");
        let mut tasks = JoinSet::new();
        // listen for incoming events
        let state_1 = state.clone();
        tasks.spawn(async move {
            let state = state_1;
            while let Some(msg) = rx_in.recv().await {
                log::debug!("RECV {msg:?}");
                if let Err(e) = Self::handle_msg_in(&state, &mut repo, msg).await {
                    log::error!("{e}");
                }
            }
        });

        // send heartbeat
        let state_2 = state.clone();
        tasks.spawn(async move {
            let state = state_2;
            loop {
                sleep(HB_SEND_DUR).await;
                if let Err(e) = state.tx_out.send(MessageOut::HeartBeat).await {
                    log::error!("Could not send HB: {e}");
                }
            }
        });

        tokio::spawn(async move {
            if let Some(_) = tasks.join_next().await {
                log::error!("Terminating Game Actor");
                tasks.abort_all();
            }
        });
    }

    async fn handle_msg_in(
        state: &ActorState,
        repo: &mut Connect4Repo,
        msg: MessageIn,
    ) -> Result<(), ActorSendError> {
        match msg {
            MessageIn::NewGame { users } => {
                let game0 = repo.get_game_id(&users.0);
                let game1 = repo.get_game_id(&users.1);
                if game0.is_some() || game1.is_some() {
                    let resp = match (game0, game1) {
                        (None, None) => unreachable!(),
                        (None, Some(_)) => MessageOut::UserAlreadyInGame {
                            reject_users: vec![users.1],
                            rematchmake_users: vec![users.0],
                        },
                        (Some(_), None) => MessageOut::UserAlreadyInGame {
                            reject_users: vec![users.0],
                            rematchmake_users: vec![users.1],
                        },
                        (Some(_), Some(_)) => MessageOut::UserAlreadyInGame {
                            reject_users: vec![users.0, users.1],
                            rematchmake_users: vec![],
                        },
                    };
                    state.tx_out.send(resp).await?;
                    return Ok(());
                }
                let game_id = repo.create_new_game(users.clone());
                let game = repo.get_game(game_id).expect("Game was just created...");
                log::info!("New game '{game_id}' created: {} vs {}", users.0, users.1);
                state
                    .tx_out
                    .send(MessageOut::NewGame {
                        game_id,
                        users: game.users.clone(),
                        first_turn: game.user_first.clone(),
                    })
                    .await?;
            }
            MessageIn::Move { user, col } => {
                let Some(game_id) = repo.get_game_id(&user) else {
                    state
                        .tx_out
                        .send(MessageOut::UserNotInGame { user })
                        .await?;
                    return Ok(());
                };
                let Some(game) = repo.get_game(game_id) else {
                    log::error!("No game object with game id: {game_id}");
                    return Ok(());
                };
                let valid = game.play(&user, col);
                let rival = game.get_rival(&user);
                state
                    .tx_out
                    .send(MessageOut::MoveValid {
                        player: user.clone(),
                        rival: rival.clone(),
                        valid,
                        col,
                    })
                    .await?;
                if log::log_enabled!(log::Level::Debug) {
                    let board = game.board_to_str();
                    log::debug!("BOARD:\n{board}\n");
                }
                if let Some(gameover) = game.is_gameover() {
                    let gameover = match gameover {
                        GameOver::Winner(winner) => {
                            let loser = game.get_rival(&winner);
                            log::info!("Game over ({game_id}). Winner: {winner}, Loser: {loser}");
                            MessageOut::GameOverWinner { winner, loser }
                        }
                        GameOver::Draw => {
                            let users = (user, rival);
                            log::info!("Game over ({game_id}). Draw: {} - {}", &users.0, &users.1);
                            MessageOut::GameOverDraw { users }
                        }
                    };
                    repo.delete_game(game_id);
                    state.tx_out.send(gameover).await?;
                }
            }
            MessageIn::UserLeft { user } => {
                if let Some(rival) = repo.delete_user(&user) {
                    log::info!("{user} left the game. Notifying {rival} to leave.");
                    state
                        .tx_out
                        .send(MessageOut::AbortGame { user: rival })
                        .await?;
                }
            }
            MessageIn::QueryGetState { respond_to } => {
                let mut users = repo.get_users();
                let n_users = users.len();
                let mut games = repo.get_game_ids();
                let n_games = games.len();
                let total_games_played = repo.total_games_played;

                if n_users > 100 {
                    users = vec![];
                }
                if n_games > 100 {
                    games = vec![];
                }

                let _ = respond_to.send(StatePayload {
                    _users_playing: users,
                    _games: games,
                    _num_games: n_games,
                    _num_users_playing: n_users,
                    _total_games_played: total_games_played,
                });
            }
        }
        Ok(())
    }
}
