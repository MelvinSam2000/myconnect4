use tokio::sync::mpsc;

use crate::actor::controller::*;
use crate::actor::service::MessageIn;
use crate::actor::service::MessageInInner;
use crate::actor::service::MessageOut;
use crate::actor::service::MessageOutInner;
use crate::actor::BUFFER_MAX;

#[tokio::test]
async fn test_normal_game() {
    let (tx_in, mut rx_in) = mpsc::channel(BUFFER_MAX);
    let (tx_out, rx_out) = mpsc::channel(BUFFER_MAX);

    let controller = ActorController::new_no_service(tx_in, rx_out);

    tokio::spawn(controller.run_all());

    let users = ("Alice".to_string(), "Bob".to_string());

    tx_out
        .send(MessageOut {
            user: users.0.clone(),
            inner: MessageOutInner::SearchGame,
        })
        .await
        .unwrap();
    tx_out
        .send(MessageOut {
            user: users.1.clone(),
            inner: MessageOutInner::SearchGame,
        })
        .await
        .unwrap();

    let res = rx_in.recv().await;
    let Some(MessageIn {
        user: user1,
        inner:
            MessageInInner::NewGame {
                game_id: gid1,
                rival: rival1,
                first_turn: fturn1,
            },
    }) = res
    else {
        panic!("user 1 did not send newgame");
    };

    let res = rx_in.recv().await;
    let Some(MessageIn {
        user: user2,
        inner:
            MessageInInner::NewGame {
                game_id: gid2,
                rival: rival2,
                first_turn: fturn2,
            },
    }) = res
    else {
        panic!("user 2 did not send newgame");
    };
    assert_ne!(user1, user2);
    assert_eq!(gid1, gid2);
    assert_ne!(fturn1, fturn2);
    assert_eq!(users.0, rival2);
    assert_eq!(users.1, rival1);

    let users = if fturn1 {
        (users.0, users.1)
    } else {
        (users.1, users.0)
    };

    for _ in 0..3 {
        // move 1
        tx_out
            .send(MessageOut {
                user: users.0.clone(),
                inner: MessageOutInner::Move { col: 0 },
            })
            .await
            .unwrap();

        let resp = rx_in.recv().await.unwrap();
        assert_eq!(
            resp,
            MessageIn {
                user: users.0.clone(),
                inner: MessageInInner::MoveValid { valid: true }
            }
        );
        let resp = rx_in.recv().await.unwrap();
        assert_eq!(
            resp,
            MessageIn {
                user: users.1.clone(),
                inner: MessageInInner::RivalMove { col: 0 }
            }
        );
        // move 2
        tx_out
            .send(MessageOut {
                user: users.1.clone(),
                inner: MessageOutInner::Move { col: 1 },
            })
            .await
            .unwrap();

        let resp = rx_in.recv().await.unwrap();
        assert_eq!(
            resp,
            MessageIn {
                user: users.1.clone(),
                inner: MessageInInner::MoveValid { valid: true }
            }
        );
        let resp = rx_in.recv().await.unwrap();
        assert_eq!(
            resp,
            MessageIn {
                user: users.0.clone(),
                inner: MessageInInner::RivalMove { col: 1 }
            }
        );
    }
    // final move
    // move 1
    tx_out
        .send(MessageOut {
            user: users.0.clone(),
            inner: MessageOutInner::Move { col: 0 },
        })
        .await
        .unwrap();

    let resp = rx_in.recv().await.unwrap();
    assert_eq!(
        resp,
        MessageIn {
            user: users.0.clone(),
            inner: MessageInInner::MoveValid { valid: true }
        }
    );
    let resp = rx_in.recv().await.unwrap();
    assert_eq!(
        resp,
        MessageIn {
            user: users.1.clone(),
            inner: MessageInInner::RivalMove { col: 0 }
        }
    );
    // game over
    let resp = rx_in.recv().await.unwrap();
    assert_eq!(
        resp,
        MessageIn {
            user: users.0.clone(),
            inner: MessageInInner::GameOver { won: Some(true) }
        }
    );
    let resp = rx_in.recv().await.unwrap();
    assert_eq!(
        resp,
        MessageIn {
            user: users.1.clone(),
            inner: MessageInInner::GameOver { won: Some(false) }
        }
    );
}
