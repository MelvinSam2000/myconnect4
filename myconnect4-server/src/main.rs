pub mod myconnect4 {
    tonic::include_proto!("myconnect4");
}
pub mod actor;
pub mod game;
pub mod repo;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use actor::controller;
use actor::controller::MainControllerActor;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_stream::Stream;
use tokio_stream::StreamExt;
use tonic::transport::Server;
use tonic::Code;
use tonic::Request;
use tonic::Response;
use tonic::Status;

use crate::actor::controller::MessageResponse;
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
use crate::myconnect4::User;
use crate::myconnect4::UserValid;
use crate::myconnect4::Winner;

const BUFFER_CHANNEL_MAX: usize = 100;

pub struct MyConnect4ServiceImpl {
    tx_req: mpsc::Sender<(String, controller::MessageRequest)>,
    map_tx_resp: Arc<Mutex<HashMap<String, mpsc::Sender<controller::MessageResponse>>>>,
}

impl MyConnect4ServiceImpl {
    fn init() -> Self {
        let controller_actor = MainControllerActor::new();
        let tx_req = controller_actor.get_sender();
        let map_tx_resp = controller_actor.get_tx_map();
        controller_actor.start();
        Self {
            tx_req,
            map_tx_resp,
        }
    }
}

#[tonic::async_trait]
impl MyConnect4Service for MyConnect4ServiceImpl {
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

        /*
        if !self.0.lock().await.repo.validate_user(&user) {
            log::warn!("Rejected a connection with invalid user: {user}");
            return Err(Status::new(Code::Cancelled, "The username is invalid."));
        }
        */

        log::info!("User {user} joined.");

        let mut stream_in = request.into_inner();

        let (tx, mut rx) = tokio::sync::mpsc::channel(BUFFER_CHANNEL_MAX);
        let (tx_resp, mut rx_resp) = mpsc::channel(BUFFER_CHANNEL_MAX);

        self.map_tx_resp.lock().await.insert(user.clone(), tx_resp);

        log::debug!("Registered user channel for {user}");

        let tx_req = self.tx_req.clone();

        tokio::spawn(async move {
            while let Some(msg) = rx_resp.recv().await {
                match msg {
                    MessageResponse::NewGame {
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
                    MessageResponse::MoveValid { valid } => tx
                        .send(GameEvent {
                            event: Some(Event::MoveValid(MoveValid { valid })),
                        })
                        .await
                        .unwrap(),
                    MessageResponse::RivalMove { col } => tx
                        .send(GameEvent {
                            event: Some(Event::Move(Move { col: col as u32 })),
                        })
                        .await
                        .unwrap(),
                    MessageResponse::GameOver { won } => {
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
                    MessageResponse::RivalLeft => {
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
                        log::info!("{user} left.");
                        tx_req
                            .send((user.clone(), controller::MessageRequest::UserLeft))
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
                        let col = col as usize;
                        tx_req
                            .send((user, controller::MessageRequest::Move { col }))
                            .await
                            .unwrap();
                    }
                    Event::SearchGame(_) => tx_req
                        .send((user, controller::MessageRequest::SearchGame))
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

    async fn validate_username(&self, _: Request<User>) -> Result<Response<UserValid>, Status> {
        // TODO: Fix this method
        Ok(Response::new(UserValid { valid: true }))
    }

    async fn query_state(&self, _: Request<Empty>) -> Result<Response<ServerState>, Status> {
        Ok(Response::new(ServerState {
            response: "State is not available rn".to_string(),
        }))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let addr = "127.0.0.1:50050".parse().expect("Invalid address provided");
    let service = MyConnect4ServiceImpl::init();

    log::debug!("DEBUG logs enabled");
    log::info!("Server listening on {addr}");

    Server::builder()
        .add_service(MyConnect4ServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
