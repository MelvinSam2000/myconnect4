pub mod myconnect4 {
    tonic::include_proto!("myconnect4");
}
pub mod actor;
pub mod game;
pub mod repo;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use log::log_enabled;
use myconnect4::Winner;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_stream::Stream;
use tokio_stream::StreamExt;
use tonic::transport::Server;
use tonic::Code;
use tonic::Request;
use tonic::Response;
use tonic::Status;

use crate::myconnect4::game_event;
use crate::myconnect4::game_event::Event;
use crate::myconnect4::game_over::Kind;
use crate::myconnect4::my_connect4_service_server::MyConnect4Service;
use crate::myconnect4::my_connect4_service_server::MyConnect4ServiceServer;
use crate::myconnect4::Empty;
use crate::myconnect4::GameEvent;
use crate::myconnect4::Move;
use crate::myconnect4::MoveValid;
use crate::myconnect4::NewGame;
use crate::myconnect4::ServerState;
use crate::myconnect4::User;
use crate::myconnect4::UserValid;
use crate::repo::Connect4Repo;

const BUFFER_CHANNEL_MAX: usize = 100;

pub struct MyConnect4ServiceImpl(Arc<Mutex<ServerCore>>);

#[derive(Default, Debug)]
struct ServerCore {
    clients: HashMap<String, Sender<GameEvent>>,
    repo: Connect4Repo,
    search_queue: Vec<String>,
}

impl MyConnect4ServiceImpl {
    fn init() -> Self {
        let server = Arc::new(Mutex::new(ServerCore::default()));
        Self::start_search_queue_task(server.clone());
        Self(server)
    }

    async fn register_client(&self, user: &str, channel: Sender<GameEvent>) {
        let clients = &mut self.0.lock().await.clients;
        clients.insert(user.to_string(), channel);
    }

    async fn server_send(
        clients: &HashMap<String, Sender<GameEvent>>,
        dst: &str,
        event: game_event::Event,
    ) {
        let Some(channel) = clients.get(dst) else {
            log::warn!("Server does not have a channel for user:{dst}");
            return;
        };
        if channel
            .send(GameEvent {
                event: Some(event.clone()),
            })
            .await
            .is_err()
        {
            log::warn!("Channel failed to send {event:?} to {dst}. Discarding event.");
        }
    }

    fn start_search_queue_task(server: Arc<Mutex<ServerCore>>) {
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_millis(1000)).await;
                let mut server = server.lock().await;
                if server.search_queue.len() >= 2 {
                    let user1 = server.search_queue.pop().expect("SQ has len >= 2");
                    let user2 = server.search_queue.pop().expect("SQ has len >= 2");

                    let game_id = server.repo.create_new_game((user1.clone(), user2.clone()));
                    let user_first = server
                        .repo
                        .get_game_mut(game_id)
                        .expect("Game ID was just created yet it doesnt exist ?!")
                        .user_first
                        .clone();

                    log::info!("New game '{game_id}' created: {user1} vs {user2}");

                    Self::server_send(
                        &server.clients,
                        &user1,
                        Event::NewGame(NewGame {
                            game_id,
                            rival: user2.clone(),
                            first_turn: user_first == user1,
                        }),
                    )
                    .await;
                    Self::server_send(
                        &server.clients,
                        &user2,
                        Event::NewGame(NewGame {
                            game_id,
                            rival: user1,
                            first_turn: user_first == user2,
                        }),
                    )
                    .await;
                }
            }
        });
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

        if !self.0.lock().await.repo.validate_user(&user) {
            log::warn!("Rejected a connection with invalid user: {user}");
            return Err(Status::new(Code::Cancelled, "The username is invalid."));
        }
        self.0.lock().await.repo.create_user(&user);

        log::info!("User {user} joined.");

        let mut stream_in = request.into_inner();

        let (tx, mut rx) = tokio::sync::mpsc::channel(BUFFER_CHANNEL_MAX);
        self.register_client(&user, tx).await;

        let server = self.0.clone();

        tokio::spawn(async move {
            while let Some(evt) = stream_in.next().await {
                let evt = match evt {
                    Ok(evt) => evt,
                    Err(err_status) => {
                        log::info!("{user} left.");
                        let mut server = server.lock().await;
                        if let Some(rival) = server.repo.delete_user(&user) {
                            Self::server_send(&server.clients, &rival, Event::RivalLeft(Empty {}))
                                .await;
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
                log::debug!("Received {event:?} from {user}");
                match event {
                    Event::Move(Move { col }) => {
                        let mut server = server.lock().await;
                        let Some(game_id) = server.repo.get_game_id(&user) else {
                            log::warn!("User {user} is not in a game right now. Ignoring.");
                            continue;
                        };
                        let Some(game) = server.repo.get_game_mut(game_id) else {
                            log::error!(
                                "User {user} has a game ID but game ID does not have a game. \
                                 Ignoring."
                            );
                            continue;
                        };
                        let valid = game.play(&user, col as usize);
                        //let game_over = game.is_gameover();
                        let users = game.users.clone();
                        let rival = if user == users.0 { &users.1 } else { &users.0 };

                        if log_enabled!(log::Level::Debug) {
                            let board = game.board_to_str();
                            log::debug!("BOARD:\n{board}\n");
                        }

                        Self::server_send(
                            &server.clients,
                            &user,
                            Event::MoveValid(MoveValid { valid }),
                        )
                        .await;
                        if !valid {
                            continue;
                        }
                        Self::server_send(&server.clients, rival, Event::Move(Move { col })).await;
                    }
                    Event::SearchGame(_) => {
                        let search_queue = &mut server.lock().await.search_queue;
                        if !search_queue.contains(&user) {
                            search_queue.push(user.clone());
                        } else {
                            log::warn!("{user} sent search game event twice!");
                        }
                    }
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

    async fn validate_username(
        &self,
        request: Request<User>,
    ) -> Result<Response<UserValid>, Status> {
        let user = request.into_inner().user;
        let valid = self.0.lock().await.repo.validate_user(&user);
        if !valid {
            log::warn!("Invalid username '{user}' tried to join.");
        }
        Ok(Response::new(UserValid { valid }))
    }

    async fn query_state(&self, _: Request<Empty>) -> Result<Response<ServerState>, Status> {
        let server = self.0.lock().await;
        Ok(Response::new(ServerState {
            response: format!("REPO: {:?} SQUEUE {:?}", server.repo, server.search_queue),
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
