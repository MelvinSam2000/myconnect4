pub mod myconnect4 {
    tonic::include_proto!("myconnect4");
}
pub mod game;
pub mod repo;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use log::log_enabled;
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
use crate::myconnect4::my_connect4_service_server::MyConnect4Service;
use crate::myconnect4::my_connect4_service_server::MyConnect4ServiceServer;
use crate::myconnect4::GameEvent;
use crate::myconnect4::Move;
use crate::myconnect4::MoveValid;
use crate::myconnect4::NewGame;
use crate::repo::Connect4Repo;

const BUFFER_CHANNEL_MAX: usize = 100;

pub struct MyConnect4ServiceImpl(Arc<Mutex<ServerCore>>);

#[derive(Default)]
struct ServerCore {
    clients: HashMap<String, Sender<GameEvent>>,
    repo: Connect4Repo,
    search_queue: Vec<String>,
}

impl MyConnect4ServiceImpl {
    async fn init() -> Self {
        let server = Arc::new(Mutex::new(ServerCore::default()));
        Self::start_search_queue_task(server.clone()).await;
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
        let channel = match clients.get(dst) {
            Some(channel) => channel,
            None => {
                log::warn!("Server does not have a channel for user:{dst}");
                return;
            }
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

    async fn start_search_queue_task(server: Arc<Mutex<ServerCore>>) {
        let server = server.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_millis(100)).await;
                let mut server = server.lock().await;
                if server.search_queue.len() >= 2 {
                    let user1 = server.search_queue.pop().expect("SQ has len >= 2");
                    let user2 = server.search_queue.pop().expect("SQ has len >= 2");

                    let game_id = server.repo.create_new_game([user1.clone(), user2.clone()]);
                    let user_first = server
                        .repo
                        .get_game_mut(game_id)
                        .expect("Game ID was just created yet it doesnt exist ?!")
                        .user_first
                        .clone();

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

        log::debug!("New stream request from {user}");

        let mut stream_in = request.into_inner();

        let (tx, mut rx) = tokio::sync::mpsc::channel(BUFFER_CHANNEL_MAX);
        self.register_client(&user, tx).await;

        let server = self.0.clone();

        //while let Some(evt) = request_rx.recv().await {
        tokio::spawn(async move {
            while let Some(evt) = stream_in.next().await {
                let evt = match evt {
                    Ok(evt) => evt,
                    Err(err_status) => {
                        log::info!("Connection with {user} closed.");
                        match err_status.code() {
                            Code::Cancelled => {}
                            code @ _ => {
                                log::error!("Connection with {user} dropped due to: {code}");
                            }
                        }
                        continue;
                    }
                };

                let GameEvent { event } = evt;
                let event = match event {
                    Some(event) => event,
                    None => {
                        log::warn!("User {user} sent an empty event. Ignoring.");
                        continue;
                    }
                };
                log::debug!("Received {event:?} from {user}");
                match event {
                    Event::Move(Move { col }) => {
                        let mut server = server.lock().await;
                        let game_id = match server.repo.get_game_id(&user) {
                            Some(game_id) => game_id,
                            None => {
                                log::warn!("User {user} is not in a game right now. Ignoring.");
                                continue;
                            }
                        };
                        let game = match server.repo.get_game_mut(game_id) {
                            Some(game) => game,
                            None => {
                                log::error!(
                                    "User {user} has a game ID but game ID does not have a game. \
                                     Ignoring."
                                );
                                continue;
                            }
                        };
                        let valid = game.play(&user, col as usize);
                        let game_over = game.is_gameover();
                        let users = game.users.clone();
                        let rival = if user == users[0] {
                            &users[1]
                        } else {
                            &users[0]
                        };

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
                        if let Some(game_over) = game_over {
                            Self::server_send(
                                &server.clients,
                                &users[0],
                                Event::GameOver(game_over.clone()),
                            )
                            .await;
                            Self::server_send(
                                &server.clients,
                                &users[1],
                                Event::GameOver(game_over),
                            )
                            .await;
                        }
                    }
                    Event::SearchGame(_) => {
                        let search_queue = &mut server.lock().await.search_queue;
                        if !search_queue.contains(&user) {
                            search_queue.push(user.clone());
                        } else {
                            log::warn!("{user} sent search game event twice!");
                        }
                    }
                    Event::GameOver(_) | Event::NewGame(_) | Event::MoveValid(_) => {
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let addr = "127.0.0.1:50050".parse().expect("Invalid address provided");
    let service = MyConnect4ServiceImpl::init().await;

    log::debug!("DEBUG logs enabled");
    log::info!("Server listening on {addr}");

    Server::builder()
        .add_service(MyConnect4ServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
