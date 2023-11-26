use std::io;
use std::io::Write;

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
use crate::myconnect4::User;

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
    /// query server state
    #[arg(short)]
    pub query: bool,
}

fn cmd_to_evt(cmd: &str) -> Option<Event> {
    let regex_search = Regex::new(r"^s(earch)?$").ok()?;
    let regex_move = Regex::new(r"^m(ove)? (\d+)$").ok()?;

    if regex_search.is_match(cmd) {
        Some(Event::SearchGame(Empty {}))
    } else if let Some(caps) = regex_move.captures(cmd) {
        caps.get(2)
            .and_then(|m| m.as_str().parse::<u32>().ok())
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
    let query_state = args.query;

    let mut client = MyConnect4ServiceClient::connect(address.clone())
        .await
        .expect("Client could not connect to server");

    if query_state {
        let state_resp = client
            .query_state(Request::new(Empty {}))
            .await
            .expect("Failed to query server state")
            .into_inner()
            .response;
        println!("{state_resp}");
        return;
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    let cli_task = async move {
        loop {
            print!("command# ");
            let _ = io::stdout().flush();
            let mut cmd = String::new();
            tokio::io::stdin().read_to_string(&mut cmd).await.unwrap();
            println!();

            let Some(cmd) = cmd_to_evt(&cmd) else {
                println!("Invalid command entered!");
                continue;
            };
            println!("SEND {cmd:?}");
            tx.send(GameEvent { event: Some(cmd) }).await.unwrap();
        }
    };

    let outbound = async_stream::stream! {
        while let Some(evt) = rx.recv().await {
            yield evt;
        }
    };

    let user_valid_response = client
        .validate_username(Request::new(User { user: user.clone() }))
        .await;
    let user_valid = user_valid_response
        .expect("Could not validate user with server.")
        .into_inner()
        .valid;
    if !user_valid {
        eprintln!("Server has rejected this username");
        return;
    }

    let mut request = Request::new(outbound);
    request.metadata_mut().insert(
        "user",
        user.clone()
            .parse()
            .expect("Client entered an unparseable username"),
    );
    let response = client
        .stream_events(request)
        .await
        .expect("Cannot connect to server!");
    let mut stream = response.into_inner();
    println!("Client successfully connected to {address}");

    tokio::spawn(cli_task);

    tokio::spawn(async move {
        while let Some(evt) = stream.next().await {
            match evt {
                Ok(evt) => println!("RECV {evt:?}"),
                Err(_) => println!("Disconnected from server"),
            }
        }
    })
    .await
    .unwrap();
}
