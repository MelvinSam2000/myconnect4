use crate::game::Connect4Game;
use crate::minimax;

#[test]
fn test_obvious_best_move() {
    #[rustfmt::skip]
    let tcases = vec![
        (
            ".......\n\
             .......\n\
             .......\n\
             ....T..\n\
             ...TTFT\n\
             TFTFTFT\n",
            Some(4)
        ),
        (
            ".......\n\
             .......\n\
             .......\n\
             ....F..\n\
             ...TFFT\n\
             TFTFFFT\n",
            Some(4)
        ),
        (
            ".......\n\
             .......\n\
             .......\n\
             .F..TF.\n\
             .F.TFTT\n\
             TFTFFFT\n",
            Some(5)
        ),
        (
            ".......\n\
             .......\n\
             .T.....\n\
             .F..TF.\n\
             .F.FFTT\n\
             TFTFFFT\n",
            Some(2)
        ),
    ];
    let users = ("Alice".to_owned(), "Bob".to_owned());
    for (i, (board, solution)) in tcases.into_iter().enumerate() {
        let mut game = Connect4Game::new_custom(0, users.0.clone(), users.1.clone(), true);
        game.board_from_str(board);
        let best_move = minimax::best_move(&mut game);
        assert_eq!(best_move, solution, "Failure at testcase#{i}");
    }
}
