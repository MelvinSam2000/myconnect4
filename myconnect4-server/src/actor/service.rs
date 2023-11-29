use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tokio_stream::Stream;
use tokio_stream::StreamExt;
use tonic::transport::Server;
use tonic::Code;
use tonic::Request;
use tonic::Response;
use tonic::Status;

use super::game;
use super::matchmaking;
use super::BUFFER_MAX;
use crate::myconnect4::game_event::Event;
use crate::myconnect4::game_over::Kind;
use crate::myconnect4::my_connect4_service_server::MyConnect4Service;
use crate::myconnect4::my_connect4_service_server::MyConnect4ServiceServer;
use crate::myconnect4::Empty;
use crate::myconnect4::GameEvent;
use crate::myconnect4::GameOver;
use crate::myconnect4::Move;
use crate::myconnect4::MoveValid;
use crate::myconnect4::NewGame;
use crate::myconnect4::ServerState;
use crate::myconnect4::Winner;

#[derive(Debug, PartialEq, Eq)]
pub struct MessageIn {
    pub user: String,
    pub inner: MessageInInner,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageInInner {
    NewGame {
        game_id: u64,
        rival: String,
        first_turn: bool,
    },
    MoveValid {
        valid: bool,
    },
    RivalMove {
        col: u8,
    },
    GameOver {
        won: Option<bool>,
    },
    RivalLeft,
}

#[derive(Debug)]
pub struct MessageOut {
    pub user: String,
    pub inner: MessageOutInner,
}

#[derive(Debug)]
pub enum MessageOutInner {
    SearchGame,
    Move {
        col: u8,
    },
    UserLeft,
    QueryGetState {
        respond_to: oneshot::Sender<(matchmaking::StatePayload, game::StatePayload)>,
    },
}

pub struct ServiceActor {
    tx_in: Sender<MessageIn>,
    tx_out: Sender<MessageOut>,
    rx_in: Option<Receiver<MessageIn>>,
    map_clients_out: Arc<RwLock<HashMap<String, Sender<MessageInInner>>>>,
}

impl ServiceActor {
    pub fn new(tx_out: Sender<MessageOut>) -> Self {
        let (tx_in, rx_in) = mpsc::channel(BUFFER_MAX);
        Self {
            tx_in,
            tx_out,
            rx_in: Some(rx_in),
            map_clients_out: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_sender(&self) -> Sender<MessageIn> {
        self.tx_in.clone()
    }

    pub fn start(mut self) {
        let map_clients_out = self.map_clients_out.clone();
        let rx_in = self.rx_in.take();

        // grpc server
        tokio::spawn(async move {
            let addr = "127.0.0.1:50050".parse().expect("Invalid address provided");

            log::info!("Server listening on {addr}");

            Server::builder()
                .add_service(MyConnect4ServiceServer::new(self))
                .serve(addr)
                .await
                .unwrap();
        });

        // message rerouting
        tokio::spawn(async move {
            let mut rx_in = rx_in.unwrap();
            while let Some(msg) = rx_in.recv().await {
                let MessageIn { user, inner: msg } = msg;
                if let Some(tx) = map_clients_out.read().await.get(&user) {
                    tx.send(msg).await.unwrap();
                }
            }
        });
    }
}

#[tonic::async_trait]
impl MyConnect4Service for ServiceActor {
    type StreamEventsStream =
        Pin<Box<dyn Stream<Item = Result<GameEvent, Status>> + Send + 'static>>;

    async fn stream_events(
        &self,
        request: Request<tonic::Streaming<GameEvent>>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let user = request
            .metadata()
            .get("user")
            .ok_or(Status::new(
                Code::InvalidArgument,
                "Client did not provide a 'user' header",
            ))?
            .to_str()
            .map_err(|_| {
                Status::new(
                    Code::InvalidArgument,
                    "The 'user' header provided uses invalid characters",
                )
            })?
            .to_string();

        if self.map_clients_out.read().await.contains_key(&user) {
            log::warn!("Rejected a connection with invalid user: {user}");
            return Err(Status::new(Code::Cancelled, "The username is taken."));
        }

        log::info!("User {user} joined.");

        let mut stream_in = request.into_inner();

        let (tx, mut rx) = tokio::sync::mpsc::channel(BUFFER_MAX);
        let (tx_in, mut rx_in) = mpsc::channel(BUFFER_MAX);

        self.map_clients_out
            .write()
            .await
            .insert(user.clone(), tx_in);

        log::debug!("Registered user channel for {user}");

        let tx_out = self.tx_out.clone();

        let map_clients_out = self.map_clients_out.clone();

        tokio::spawn(async move {
            while let Some(msg) = rx_in.recv().await {
                match msg {
                    MessageInInner::NewGame {
                        game_id,
                        rival,
                        first_turn,
                    } => tx
                        .send(GameEvent {
                            event: Some(Event::NewGame(NewGame {
                                game_id,
                                rival,
                                first_turn,
                            })),
                        })
                        .await
                        .unwrap(),
                    MessageInInner::MoveValid { valid } => tx
                        .send(GameEvent {
                            event: Some(Event::MoveValid(MoveValid { valid })),
                        })
                        .await
                        .unwrap(),
                    MessageInInner::RivalMove { col } => tx
                        .send(GameEvent {
                            event: Some(Event::Move(Move { col: col as u32 })),
                        })
                        .await
                        .unwrap(),
                    MessageInInner::GameOver { won } => {
                        let gameover = match won {
                            Some(user_won) => Event::GameOver(GameOver {
                                kind: Some(Kind::Winner(Winner { user_won })),
                            }),
                            None => Event::GameOver(GameOver {
                                kind: Some(Kind::Draw(Empty {})),
                            }),
                        };
                        tx.send(GameEvent {
                            event: Some(gameover),
                        })
                        .await
                        .unwrap();
                    }
                    MessageInInner::RivalLeft => {
                        tx.send(GameEvent {
                            event: Some(Event::RivalLeft(Empty {})),
                        })
                        .await
                        .unwrap();
                    }
                }
            }
        });

        tokio::spawn(async move {
            while let Some(evt) = stream_in.next().await {
                let user = user.clone();
                let evt = match evt {
                    Ok(evt) => evt,
                    Err(err_status) => {
                        let mut wmap = map_clients_out.write().await;
                        wmap.remove(&user).unwrap();
                        drop(wmap);
                        log::info!("{user} left.");
                        tx_out
                            .send(MessageOut {
                                user: user.clone(),
                                inner: MessageOutInner::UserLeft,
                            })
                            .await
                            .unwrap();
                        match err_status.code() {
                            Code::Cancelled | Code::Unknown => {}
                            code @ _ => {
                                log::error!("Connection with {user} dropped due to: {code}");
                            }
                        }
                        continue;
                    }
                };

                let GameEvent { event } = evt;
                let Some(event) = event else {
                    log::warn!("User {user} sent an empty event. Ignoring.");
                    continue;
                };
                log::debug!("Received {event:?} from {user}");
                match event {
                    Event::Move(Move { col }) => {
                        let col = col as u8;
                        tx_out
                            .send(MessageOut {
                                user,
                                inner: MessageOutInner::Move { col },
                            })
                            .await
                            .unwrap();
                    }
                    Event::SearchGame(_) => tx_out
                        .send(MessageOut {
                            user,
                            inner: MessageOutInner::SearchGame,
                        })
                        .await
                        .unwrap(),
                    Event::GameOver(_)
                    | Event::NewGame(_)
                    | Event::MoveValid(_)
                    | Event::RivalLeft(_) => {
                        log::warn!("Ignoring: {event:?}");
                    }
                }
            }
        });

        let stream_out = async_stream::try_stream! {
            while let Some(evt) = rx.recv().await {
                yield evt;
            }
        };

        Ok(Response::new(
            Box::pin(stream_out) as Self::StreamEventsStream
        ))
    }

    async fn query_state(&self, _: Request<Empty>) -> Result<Response<ServerState>, Status> {
        let (tx, rx) = oneshot::channel();
        self.tx_out
            .send(MessageOut {
                user: "*".to_string(),
                inner: MessageOutInner::QueryGetState { respond_to: tx },
            })
            .await
            .unwrap();
        let state = rx.await.unwrap();
        let users = self
            .map_clients_out
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<String>>();
        Ok(Response::new(ServerState {
            response: format!("{users:?} - {state:?}"),
        }))
    }
}
