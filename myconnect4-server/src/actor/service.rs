use std::collections::HashMap;
use std::pin::Pin;
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
use super::HB_SEND_DUR;
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
use crate::myconnect4::SpawnSeveral;
use crate::myconnect4::Winner;

const CLIENT_BUFFER_MAX: usize = 100;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct MessageIn {
    pub user: String,
    pub inner: MessageInInner,
}

#[derive(Debug, PartialEq, Eq, Clone)]
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
    SpawnSeveralBots {
        number: usize,
    },
    HeartBeat,
}

pub struct ServiceActor {
    state: ActorState,
    rx_in: Receiver<MessageIn>,
}

#[derive(Clone)]
struct ActorState {
    tx_in: Sender<MessageIn>,
    tx_out: Sender<MessageOut>,
    map_client_channels: Arc<RwLock<HashMap<String, Sender<MessageInInner>>>>,
}

#[derive(Debug, Error)]
enum ActorSendError {
    #[error("Error sending msg: {0}")]
    MessageIn(#[from] SendError<MessageIn>),
    #[error("Error sending msg: {0}")]
    MessageOut(#[from] SendError<MessageOut>),
    #[error("Error sending msg: {0}")]
    GameEvent(#[from] SendError<GameEvent>),
}

impl ServiceActor {
    pub fn new(tx_out: Sender<MessageOut>) -> Self {
        let (tx_in, rx_in) = mpsc::channel(BUFFER_MAX);
        let map_client_channels = Arc::new(RwLock::new(HashMap::new()));
        Self {
            state: ActorState {
                tx_in,
                tx_out,
                map_client_channels,
            },
            rx_in,
        }
    }

    pub fn get_sender(&self) -> Sender<MessageIn> {
        self.state.tx_in.clone()
    }

    pub fn start(self) {
        let ServiceActor { state, mut rx_in } = self;

        let mut tasks = JoinSet::new();

        // grpc server start
        let state_1 = state.clone();
        tasks.spawn(async move {
            let state = state_1;
            let addr = "127.0.0.1:50050".parse().expect("Invalid address provided");

            log::info!("Server listening on {addr}");

            if let Err(e) = Server::builder()
                .add_service(MyConnect4ServiceServer::new(state))
                .serve(addr)
                .await
            {
                log::error!("Service actor terminated due to: {e}");
            }
        });

        // message rerouting
        let state_2 = state.clone();
        tasks.spawn(async move {
            let state = state_2;
            while let Some(msg) = rx_in.recv().await {
                let MessageIn { user, inner: msg } = msg;
                let rmap = state.map_client_channels.read().await;
                let tx = rmap.get(&user).cloned();
                drop(rmap);
                if let Some(tx) = tx {
                    if let Err(e) = tx.send(msg).await {
                        log::error!("Could not route message to '{user}' due to: {e}");
                    }
                }
            }
        });

        // send heartbeat
        let state_3 = state.clone();
        tasks.spawn(async move {
            let state = state_3;
            loop {
                sleep(HB_SEND_DUR).await;
                if let Err(e) = state
                    .tx_out
                    .send(MessageOut {
                        user: "*".to_string(),
                        inner: MessageOutInner::HeartBeat,
                    })
                    .await
                {
                    log::error!("Could not send HB: {e}");
                }
            }
        });

        tokio::spawn(async move {
            if let Some(_) = tasks.join_next().await {
                log::error!("Terminating Service Actor");
                tasks.abort_all();
            }
        });
    }
}

#[tonic::async_trait]
impl MyConnect4Service for ActorState {
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

        let mut wmap = self.map_client_channels.write().await;
        if wmap.contains_key(&user) {
            log::warn!("Rejected a connection with invalid user: {user}");
            return Err(Status::new(Code::Cancelled, "The username is taken."));
        }
        let (tx, mut rx) = mpsc::channel(CLIENT_BUFFER_MAX);
        let (tx_in, mut rx_in) = mpsc::channel(CLIENT_BUFFER_MAX);
        wmap.insert(user.clone(), tx_in);
        drop(wmap);
        log::info!("User {user} joined.");

        let mut stream_in = request.into_inner();

        let tx_out = self.tx_out.clone();

        let map_client_channels = self.map_client_channels.clone();

        // handle messages from controller
        let state_1 = self.clone();
        tokio::spawn(async move {
            let state = state_1;
            while let Some(msg) = rx_in.recv().await {
                log::debug!("RECV {msg:?}");
                if let Err(e) = state.handle_msg_in_inner(&tx, msg).await {
                    log::error!("{e}");
                }
            }
        });

        // handle messages from grpc client
        let state_2 = self.clone();
        tokio::spawn(async move {
            let state = state_2;
            while let Some(evt) = stream_in.next().await {
                let user = user.clone();
                let evt = match evt {
                    Ok(evt) => evt,
                    Err(err_status) => {
                        map_client_channels.write().await.remove(&user);
                        log::info!("{user} left.");
                        if let Err(e) = tx_out
                            .send(MessageOut {
                                user: user.clone(),
                                inner: MessageOutInner::UserLeft,
                            })
                            .await
                        {
                            log::error!("{user} could not send: {e}");
                        }
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
                log::debug!("RECV {event:?} from {user}");
                if let Err(e) = state.handle_game_evt(&user, event).await {
                    log::error!("{user} could not send: {e}");
                }
            }
        });

        let stream_resp = async_stream::try_stream! {
            while let Some(evt) = rx.recv().await {
                yield evt;
            }
        };

        Ok(Response::new(
            Box::pin(stream_resp) as Self::StreamEventsStream
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
            .map_err(|e| {
                log::error!("Could not send: {e}");
                Status::new(Code::Internal, "Internal server error")
            })?;
        let state = rx.await.map_err(|e| {
            log::error!("Could not recv: {e}");
            Status::new(Code::Internal, "Internal server error")
        })?;
        let users = self
            .map_client_channels
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<String>>();
        Ok(Response::new(ServerState {
            response: format!("{users:?} - {state:?}"),
        }))
    }

    async fn spawn_several_bots(
        &self,
        req: Request<SpawnSeveral>,
    ) -> Result<Response<Empty>, Status> {
        let number = req.into_inner().number as usize;
        self.tx_out
            .send(MessageOut {
                user: "*".to_string(),
                inner: MessageOutInner::SpawnSeveralBots { number },
            })
            .await
            .map_err(|e| {
                log::error!("Could not send: {e}");
                Status::new(Code::Internal, "Internal server error")
            })?;
        Ok(Response::new(Empty {}))
    }
}

impl ActorState {
    async fn handle_msg_in_inner(
        &self,
        tx: &Sender<GameEvent>,
        msg: MessageInInner,
    ) -> Result<(), ActorSendError> {
        match msg {
            MessageInInner::NewGame {
                game_id,
                rival,
                first_turn,
            } => {
                tx.send(GameEvent {
                    event: Some(Event::NewGame(NewGame {
                        game_id,
                        rival,
                        first_turn,
                    })),
                })
                .await?;
            }
            MessageInInner::MoveValid { valid } => {
                tx.send(GameEvent {
                    event: Some(Event::MoveValid(MoveValid { valid })),
                })
                .await?;
            }
            MessageInInner::RivalMove { col } => {
                tx.send(GameEvent {
                    event: Some(Event::Move(Move { col: col as u32 })),
                })
                .await?;
            }
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
                .await?;
            }
            MessageInInner::RivalLeft => {
                tx.send(GameEvent {
                    event: Some(Event::RivalLeft(Empty {})),
                })
                .await?;
            }
        }
        Ok(())
    }

    async fn handle_game_evt(&self, user: &str, event: Event) -> Result<(), ActorSendError> {
        match event {
            Event::Move(Move { col }) => {
                let col = col as u8;
                self.tx_out
                    .send(MessageOut {
                        user: user.to_string(),
                        inner: MessageOutInner::Move { col },
                    })
                    .await?;
            }
            Event::SearchGame(_) => {
                self.tx_out
                    .send(MessageOut {
                        user: user.to_string(),
                        inner: MessageOutInner::SearchGame,
                    })
                    .await?;
            }
            Event::GameOver(_) | Event::NewGame(_) | Event::MoveValid(_) | Event::RivalLeft(_) => {
                log::warn!("Ignoring: {event:?}");
            }
        }
        Ok(())
    }
}
