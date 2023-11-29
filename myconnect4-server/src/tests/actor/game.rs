use tokio::sync::mpsc;

use crate::actor::game::*;
use crate::actor::BUFFER_MAX;
use crate::game::COLS;

#[tokio::test]
async fn test_new_game_and_user_alr_exists() {
    let (tx, mut rx) = mpsc::channel(BUFFER_MAX);
    let gactor = GameActor::new(tx);
    let tx = gactor.get_sender();
    gactor.start();

    tx.send(MessageIn::NewGame {
        users: ("Alice".to_string(), "Bob".to_string()),
    })
    .await
    .unwrap();

    let resp = rx.recv().await;
    let Some(MessageOut::NewGame {
        game_id: _,
        users: _,
        first_turn: _,
    }) = resp
    else {
        panic!("New Game test failed, did not get NewGame response");
    };

    tx.send(MessageIn::NewGame {
        users: ("Alice".to_string(), "Carl".to_string()),
    })
    .await
    .unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageOut::UserAlreadyInGame {
            reject_users: vec!["Alice".to_string()],
            rematchmake_users: vec!["Carl".to_string()]
        })
    )
}

#[tokio::test]
async fn test_move_valid_and_invalid() {
    let (tx, mut rx) = mpsc::channel(BUFFER_MAX);
    let gactor = GameActor::new(tx);
    let tx = gactor.get_sender();
    gactor.start();

    let user = "Alice".to_string();

    tx.send(MessageIn::Move {
        user: user.clone(),
        col: 1,
    })
    .await
    .unwrap();
    let resp = rx.recv().await;
    assert_eq!(resp, Some(MessageOut::UserNotInGame { user }));

    tx.send(MessageIn::NewGame {
        users: ("Alice".to_string(), "Bob".to_string()),
    })
    .await
    .unwrap();

    let resp = rx.recv().await;
    let Some(MessageOut::NewGame {
        game_id: _,
        users,
        first_turn,
    }) = resp
    else {
        panic!("New Game test failed, did not get NewGame response");
    };

    let first = first_turn;
    let second = if first == users.0 { users.1 } else { users.0 };
    tx.send(MessageIn::Move {
        user: first.clone(),
        col: 1,
    })
    .await
    .unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageOut::MoveValid {
            player: first.clone(),
            rival: second.clone(),
            valid: true,
            col: 1,
        })
    );
    tx.send(MessageIn::Move {
        user: first.clone(),
        col: 1,
    })
    .await
    .unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageOut::MoveValid {
            player: first.clone(),
            rival: second.clone(),
            valid: false,
            col: 1,
        })
    );
    tx.send(MessageIn::Move {
        user: second.clone(),
        col: COLS as u8 + 1,
    })
    .await
    .unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageOut::MoveValid {
            player: second.clone(),
            rival: first.clone(),
            valid: false,
            col: COLS as u8 + 1,
        })
    );
    tx.send(MessageIn::Move {
        user: second.clone(),
        col: 1,
    })
    .await
    .unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageOut::MoveValid {
            player: second.clone(),
            rival: first.clone(),
            valid: true,
            col: 1,
        })
    );
}

#[tokio::test]
async fn test_game_over() {
    let (tx, mut rx) = mpsc::channel(BUFFER_MAX);
    let gactor = GameActor::new(tx);
    let tx = gactor.get_sender();
    gactor.start();

    tx.send(MessageIn::NewGame {
        users: ("Alice".to_string(), "Bob".to_string()),
    })
    .await
    .unwrap();

    let resp = rx.recv().await;
    let Some(MessageOut::NewGame {
        game_id: _,
        users,
        first_turn,
    }) = resp
    else {
        panic!("New Game test failed, did not get NewGame response");
    };

    let first = first_turn;
    let second = if first == users.0 { users.1 } else { users.0 };

    for _ in 0..3 {
        tx.send(MessageIn::Move {
            user: first.clone(),
            col: 0,
        })
        .await
        .unwrap();
        let resp = rx.recv().await;
        assert_eq!(
            resp,
            Some(MessageOut::MoveValid {
                player: first.clone(),
                rival: second.clone(),
                valid: true,
                col: 0,
            })
        );
        tx.send(MessageIn::Move {
            user: second.clone(),
            col: 1,
        })
        .await
        .unwrap();
        let resp = rx.recv().await;
        assert_eq!(
            resp,
            Some(MessageOut::MoveValid {
                player: second.clone(),
                rival: first.clone(),
                valid: true,
                col: 1,
            })
        );
    }
    tx.send(MessageIn::Move {
        user: first.clone(),
        col: 0,
    })
    .await
    .unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageOut::MoveValid {
            player: first.clone(),
            rival: second.clone(),
            valid: true,
            col: 0,
        })
    );
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageOut::GameOverWinner {
            winner: first,
            loser: second
        })
    );
}

#[tokio::test]
async fn test_user_left() {
    let (tx, mut rx) = mpsc::channel(BUFFER_MAX);
    let gactor = GameActor::new(tx);
    let tx = gactor.get_sender();
    gactor.start();

    tx.send(MessageIn::NewGame {
        users: ("Alice".to_string(), "Bob".to_string()),
    })
    .await
    .unwrap();

    let resp = rx.recv().await;
    let Some(MessageOut::NewGame {
        game_id: _,
        users: _,
        first_turn: _,
    }) = resp
    else {
        panic!("New Game test failed, did not get NewGame response");
    };

    tx.send(MessageIn::UserLeft {
        user: "Bob".to_string(),
    })
    .await
    .unwrap();
    let resp = rx.recv().await;
    assert_eq!(
        resp,
        Some(MessageOut::AbortGame {
            user: "Alice".to_string()
        })
    );
}
