use tokio::sync::mpsc;

use crate::actor::controller::*;
use crate::actor::BUFFER_MAX;

#[tokio::test]
async fn test_normal_game() {
    let controller = MainControllerActor::new();
    let tx = controller.get_sender();
    let user_tx = controller.get_tx_map();
    controller.start();

    let (tx1, mut rx1) = mpsc::channel(BUFFER_MAX);
    let (tx2, mut rx2) = mpsc::channel(BUFFER_MAX);

    let users = ("Alice".to_string(), "Bob".to_string());

    user_tx.write().await.insert(users.0.clone(), tx1);
    user_tx.write().await.insert(users.1.clone(), tx2);

    tx.send((users.0.clone(), MessageRequest::SearchGame))
        .await
        .unwrap();
    tx.send((users.1.clone(), MessageRequest::SearchGame))
        .await
        .unwrap();
    let res1 = rx1.recv().await;
    let Some(MessageResponse::NewGame {
        game_id: gid1,
        rival: rival1,
        first_turn: fturn1,
    }) = res1
    else {
        panic!("rx1 did not send newgame");
    };
    let res2 = rx2.recv().await;
    let Some(MessageResponse::NewGame {
        game_id: gid2,
        rival: rival2,
        first_turn: fturn2,
    }) = res2
    else {
        panic!("rx2 did not send newgame");
    };
    assert_eq!(gid1, gid2);
    assert_ne!(fturn1, fturn2);
    assert_eq!(users.0, rival2);
    assert_eq!(users.1, rival1);

    let users = if fturn1 {
        (users.0, users.1)
    } else {
        let tmp = rx1;
        rx1 = rx2;
        rx2 = tmp;
        (users.1, users.0)
    };

    for _ in 0..3 {
        // move 1
        tx.send((users.0.clone(), MessageRequest::Move { col: 0 }))
            .await
            .unwrap();
        let resp = rx1.recv().await;
        assert_eq!(resp, Some(MessageResponse::MoveValid { valid: true }));
        let resp = rx2.recv().await;
        assert_eq!(resp, Some(MessageResponse::RivalMove { col: 0 }));
        // move 2
        tx.send((users.1.clone(), MessageRequest::Move { col: 1 }))
            .await
            .unwrap();
        let resp = rx2.recv().await;
        assert_eq!(resp, Some(MessageResponse::MoveValid { valid: true }));
        let resp = rx1.recv().await;
        assert_eq!(resp, Some(MessageResponse::RivalMove { col: 1 }));
    }
    // final move
    tx.send((users.0.clone(), MessageRequest::Move { col: 0 }))
        .await
        .unwrap();
    let resp = rx1.recv().await;
    assert_eq!(resp, Some(MessageResponse::MoveValid { valid: true }));
    let resp = rx2.recv().await;
    assert_eq!(resp, Some(MessageResponse::RivalMove { col: 0 }));
    // gameover
    let resp = rx1.recv().await;
    assert_eq!(resp, Some(MessageResponse::GameOver { won: Some(true) }));
    let resp = rx2.recv().await;
    assert_eq!(resp, Some(MessageResponse::GameOver { won: Some(false) }));
}
