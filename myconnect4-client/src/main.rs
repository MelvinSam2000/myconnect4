use clap::Parser;
use myconnect4::Move;
use regex::Regex;
use tokio::io::AsyncReadExt;
use tokio_stream::StreamExt;
use tonic::Request;

use crate::myconnect4::game_event::Event;
use crate::myconnect4::my_connect4_service_client::MyConnect4ServiceClient;
use crate::myconnect4::Empty;
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

fn cmd_to_evt(cmd: &str) -> Option<Event> {
    let regex_search = Regex::new(r"^s(earch)?$").ok()?;
    let regex_move = Regex::new(r"^m(ove)? (\d+)$").ok()?;

    if regex_search.is_match(cmd) {
        Some(Event::SearchGame(Empty {}))
    } else if let Some(caps) = regex_move.captures(cmd) {
        caps.get(2)
            .and_then(|m| u32::from_str_radix(m.as_str(), 10).ok())
            .map(|col| Event::Move(Move { col }))
    } else {
        None
    }
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

    tokio::spawn(async move {
        loop {
            let mut cmd = String::from("");
            log::debug!(">>> ");
            tokio::io::stdin().read_to_string(&mut cmd).await.unwrap();

            let event = match cmd_to_evt(&cmd) {
                Some(cmd) => cmd,
                None => {
                    log::error!("Invalid command entered!");
                    continue;
                }
            };
            tx.send(GameEvent { event: Some(event) }).await.unwrap();
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
