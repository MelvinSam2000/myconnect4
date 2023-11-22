pub mod myconnect4 {
    tonic::include_proto!("myconnect4");
}

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
use tokio_stream::Stream;
use tokio_stream::StreamExt;
use tonic::transport::Server;
use tonic::Request;
use tonic::Response;
use tonic::Status;

use crate::game::Connect4Game;
use crate::myconnect4::game_event;
use crate::myconnect4::game_event::Payload;
use crate::myconnect4::my_connect4_service_server::MyConnect4Service;
use crate::myconnect4::my_connect4_service_server::MyConnect4ServiceServer;
use crate::myconnect4::GameEvent;
use crate::myconnect4::NewGame;

const SERVER_USERNAME: &str = "@SERVER";

#[derive(Default)]
pub struct MyConnect4ServiceImpl {
    games: Arc<Mutex<HashMap<String, Connect4Game>>>,
    clients: Arc<Mutex<HashMap<String, Sender<GameEvent>>>>,
}

impl MyConnect4ServiceImpl {
    async fn register_client(&self, user: &str, channel: Sender<GameEvent>) {
        let mut clients = self.clients.lock().await;
        clients.insert(user.to_string(), channel);
    }

    async fn server_send(
        clients: Arc<Mutex<HashMap<String, Sender<GameEvent>>>>,
        dst: &str,
        payload: game_event::Payload,
    ) {
        let clients = clients.lock().await;
        let channel = clients.get(dst).unwrap();
        channel
            .send(GameEvent {
                src: SERVER_USERNAME.to_string(),
                dst: dst.to_string(),
                payload: Some(payload),
            })
            .await
            .unwrap();
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
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        log::debug!("New stream request from {user}");

        let mut stream_in = request.into_inner();

        let (response_tx, mut response_rx) = tokio::sync::mpsc::channel(100);
        //let (request_tx, mut request_rx) = tokio::sync::mpsc::channel(100);
        self.register_client(&user, response_tx).await;

        let games = self.games.clone();
        let clients = self.clients.clone();

        //while let Some(evt) = request_rx.recv().await {
        tokio::spawn(async move {
            while let Some(evt) = stream_in.next().await {
                let evt = evt
                    .map_err(|err| {
                        log::info!("Connection with {user} closed.");
                        err
                    })
                    .unwrap();
                let clients = clients.clone();
                let GameEvent {
                    src: _,
                    dst: _,
                    payload,
                } = evt;
                let payload = match payload {
                    Some(payload) => payload,
                    None => {
                        log::warn!("User {user} sent an empty payload. Ignoring.");
                        continue;
                    }
                };
                log::debug!("Received {payload:?}");
                match payload {
                    Payload::Move(mov) => {
                        let col = mov.col;
                        games.lock().await.get(&user).unwrap().play(col as usize)
                    }
                    Payload::SearchGame(_) => {
                        // put user in search queue
                        Self::server_send(
                            clients,
                            &user,
                            Payload::NewGame(NewGame {
                                game_id: 0,
                                rival: "".to_string(),
                            }),
                        )
                        .await;
                    }
                    Payload::GameOver(_) | Payload::NewGame(_) => {
                        log::warn!("Ignoring: {payload:?}");
                    }
                }
            }
        });

        let stream_out = async_stream::try_stream! {
            while let Some(evt) = response_rx.recv().await {
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

    let addr = "127.0.0.1:50050".parse().unwrap();
    let service = MyConnect4ServiceImpl::default();

    log::debug!("DEBUG logs enabled");
    log::info!("Server listening on {addr}");

    Server::builder()
        .add_service(MyConnect4ServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}

pub mod game;
