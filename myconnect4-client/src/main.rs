use clap::Parser;
use tokio::io::AsyncReadExt;
use tokio_stream::StreamExt;
use tonic::Request;

use crate::myconnect4::game_event::Payload::{self};
use crate::myconnect4::my_connect4_service_client::MyConnect4ServiceClient;
use crate::myconnect4::GameEvent;

pub mod myconnect4 {
    tonic::include_proto!("myconnect4");
}

#[derive(Parser)]
#[command(about)]
pub struct Args {
    /// username to use in game
    pub username: String,
    /// server address to connect to (scheme://ip:port format)
    pub address: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let address = args.address;
    let user = args.username;

    env_logger::init();
    log::debug!("DEBUG logs enabled");

    let mut client = MyConnect4ServiceClient::connect(address.clone())
        .await
        .unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    let user_p1 = user.clone();
    tokio::spawn(async move {
        loop {
            let user_p2 = user_p1.clone();
            let mut cmd = String::from("");
            log::debug!(">>> ");
            tokio::io::stdin().read_to_string(&mut cmd).await.unwrap();
            log::debug!("Entered: {cmd}");
            match cmd.as_str() {
                "search\n" => {
                    let payload = Payload::SearchGame(myconnect4::Empty {});
                    tx.send(GameEvent {
                        src: user_p2,
                        dst: "*".to_string(),
                        payload: Some(payload),
                    })
                    .await
                    .unwrap();
                    log::debug!("Sent searchgame event");
                }
                _ => {
                    log::debug!("IGNORED");
                }
            }
        }
    });

    let outbound = async_stream::stream! {
        while let Some(evt) = rx.recv().await {
            yield evt;
        }
    };

    let mut request = Request::new(outbound);
    request
        .metadata_mut()
        .insert("user", user.clone().parse().unwrap());
    let response = client.stream_events(request).await.unwrap();
    let mut stream = response.into_inner();
    log::info!("Client successfully connected to {address}");

    tokio::spawn(async move {
        while let Some(evt) = stream.next().await {
            let evt = evt.unwrap();
            log::info!("RECV {evt:?}");
        }
    })
    .await
    .unwrap();
}
