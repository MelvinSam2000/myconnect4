use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::BUFFER_MAX;
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
    tx_in: mpsc::Sender<MessageIn>,
    tx_out: mpsc::Sender<MessageOut>,
    rx: mpsc::Receiver<MessageIn>,
    repo: Connect4Repo,
}

#[derive(Debug)]
pub struct StatePayload {
    _users_playing: Vec<String>,
    _num_users_playing: usize,
    _games: Vec<u64>,
    _num_games: usize,
}

impl GameActor {
    pub fn new(tx_out: mpsc::Sender<MessageOut>) -> Self {
        let (tx, rx) = mpsc::channel(BUFFER_MAX);
        Self {
            tx_in: tx,
            tx_out,
            rx,
            repo: Connect4Repo::default(),
        }
    }

    pub fn get_sender(&self) -> mpsc::Sender<MessageIn> {
        self.tx_in.clone()
    }

    pub fn start(mut self) {
        log::debug!("Game actor started.");
        tokio::spawn(async move {
            while let Some(req) = self.rx.recv().await {
                log::debug!("RECV {req:?}");
                log::trace!("start");
                match req {
                    MessageIn::NewGame { users } => {
                        let game0 = self.repo.get_game_id(&users.0);
                        let game1 = self.repo.get_game_id(&users.1);
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
                            self.tx_out.send(resp).await.unwrap();
                            continue;
                        }
                        let game_id = self.repo.create_new_game(users.clone());
                        let game = self
                            .repo
                            .get_game(game_id)
                            .expect("Game was just created...");
                        log::info!("New game '{game_id}' created: {} vs {}", users.0, users.1);
                        self.tx_out
                            .send(MessageOut::NewGame {
                                game_id,
                                users: game.users.clone(),
                                first_turn: game.user_first.clone(),
                            })
                            .await
                            .unwrap();
                    }
                    MessageIn::Move { user, col } => {
                        let Some(game_id) = self.repo.get_game_id(&user) else {
                            self.tx_out
                                .send(MessageOut::UserNotInGame { user })
                                .await
                                .unwrap();
                            continue;
                        };
                        let Some(game) = self.repo.get_game(game_id) else {
                            log::error!("No game object with game id: {game_id}");
                            continue;
                        };
                        let valid = game.play(&user, col);
                        let rival = game.get_rival(&user);
                        self.tx_out
                            .send(MessageOut::MoveValid {
                                player: user.clone(),
                                rival: rival.clone(),
                                valid,
                                col,
                            })
                            .await
                            .unwrap();
                        if log::log_enabled!(log::Level::Debug) {
                            let board = game.board_to_str();
                            log::debug!("BOARD:\n{board}\n");
                        }
                        if let Some(gameover) = game.is_gameover() {
                            let gameover = match gameover {
                                GameOver::Winner(winner) => {
                                    let loser = game.get_rival(&winner);
                                    log::info!(
                                        "Game over ({game_id}). Winner: {winner}, Loser: {loser}"
                                    );
                                    MessageOut::GameOverWinner { winner, loser }
                                }
                                GameOver::Draw => {
                                    let users = (user, rival);
                                    log::info!(
                                        "Game over ({game_id}). Draw: {} - {}",
                                        &users.0,
                                        &users.1
                                    );
                                    MessageOut::GameOverDraw { users }
                                }
                            };
                            self.repo.delete_game(game_id);
                            self.tx_out.send(gameover).await.unwrap();
                        }
                    }
                    MessageIn::UserLeft { user } => {
                        if let Some(rival) = self.repo.delete_user(&user) {
                            log::info!("{user} left the game. Notifying {rival} to leave.");
                            self.tx_out
                                .send(MessageOut::AbortGame { user: rival })
                                .await
                                .unwrap();
                        }
                    }
                    MessageIn::QueryGetState { respond_to } => {
                        let users = self.repo.get_users();
                        let n_users = users.len();
                        let games = self.repo.get_game_ids();
                        let n_games = games.len();
                        respond_to
                            .send(StatePayload {
                                _users_playing: users,
                                _num_users_playing: n_users,
                                _games: games,
                                _num_games: n_games,
                            })
                            .unwrap();
                    }
                }
                log::trace!("end");
            }
        });
    }
}
