use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::actor::matchmaking::*;
use crate::actor::BUFFER_MAX;

#[tokio::test]
async fn test_normal_matchmaking() {
    let (tx, mut rx) = mpsc::channel(BUFFER_MAX);
    let mmactor = MatchMakingActor::new(tx, None);
    let tx = mmactor.get_sender();
    mmactor.start();

    let users = ("Alice".to_string(), "Bob".to_string());

    tx.send(MessageRequest::Search {
        user: users.0.clone(),
    })
    .await
    .unwrap();

    tx.send(MessageRequest::DebugGetQueue).await.unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageResponse::DebugGetQueueResponse {
            queue: vec!["Alice".to_string()]
        })
    );

    tx.send(MessageRequest::Search {
        user: users.1.clone(),
    })
    .await
    .unwrap();
    let resp = rx.recv().await;
    assert_eq!(resp, Some(MessageResponse::UsersFound { users }));

    tx.send(MessageRequest::DebugGetQueue).await.unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageResponse::DebugGetQueueResponse { queue: vec![] })
    );
}

#[tokio::test]
async fn test_cancel_search() {
    let (tx, mut rx) = mpsc::channel(BUFFER_MAX);
    let mmactor = MatchMakingActor::new(tx, None);
    let tx = mmactor.get_sender();
    mmactor.start();

    let user = String::from("Alice");

    tx.send(MessageRequest::Search { user: user.clone() })
        .await
        .unwrap();
    tx.send(MessageRequest::CancelSearch { user })
        .await
        .unwrap();
    tx.send(MessageRequest::DebugGetQueue).await.unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageResponse::DebugGetQueueResponse { queue: vec![] })
    );
}

#[tokio::test]
async fn test_duplicate_search() {
    let (tx, mut rx) = mpsc::channel(BUFFER_MAX);
    let mmactor = MatchMakingActor::new(tx, None);
    let tx = mmactor.get_sender();
    mmactor.start();

    let user = String::from("Alice");

    tx.send(MessageRequest::Search { user: user.clone() })
        .await
        .unwrap();
    tx.send(MessageRequest::Search { user: user.clone() })
        .await
        .unwrap();
    tx.send(MessageRequest::DebugGetQueue).await.unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageResponse::DebugGetQueueResponse { queue: vec![user] })
    );
}

#[tokio::test]
async fn test_long_wait() {
    const TEST_WAIT_LIMIT: Duration = Duration::from_micros(10);
    let (tx, mut rx) = mpsc::channel(BUFFER_MAX);
    let mmactor = MatchMakingActor::new(tx, Some(TEST_WAIT_LIMIT));
    let tx = mmactor.get_sender();
    mmactor.start();

    let user = String::from("Alice");

    tx.send(MessageRequest::Search { user: user.clone() })
        .await
        .unwrap();
    sleep(TEST_WAIT_LIMIT * 2).await;
    let resp = rx.recv().await;
    assert_eq!(resp, Some(MessageResponse::LongWait { user }));
}
