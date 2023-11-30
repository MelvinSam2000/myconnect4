use crate::game::Connect4Game;

#[test]
fn test_draw() {
    #[rustfmt::skip]
    let positive_test_cases = vec![

        "FTFTFTF\n\
        TFTFTFT\n\
        FTFTFTF\n\
        TFTFTFT\n\
        FTFTFTF\n\
        TFTFTFT",

        "TTTFFFT\n\
        FTTTFFF\n\
        TFTFTFT\n\
        FTFTFTF\n\
        TFTFTFT\n\
        FTFTFFF",

        "FFTTFTT\n\
        TTFFTFF\n\
        FTFTFTF\n\
        TFTFTFT\n\
        FTFTFTF\n\
        TFTFTFT",

        "FTFTFTF\n\
        TFTFTFT\n\
        FTFTFTF\n\
        TFTFTFT\n\
        FTFTFTF\n\
        TFTFTFT",
    ];

    #[rustfmt::skip]
    let negative_test_cases = vec![
        ".......\n\
        .......\n\
        .......\n\
        .......\n\
        .......\n\
        .......",

        ".TFTFTF\n\
        .FTFTFT\n\
        .TFTFTF\n\
        .FTFTFT\n\
        .TFTFTF\n\
        .FTFTFT",

        "FF.....\n\
        TTFFTFF\n\
        FTFTFTF\n\
        TFTFTFT\n\
        FTFTFTF\n\
        TFTFTFT",

        "FTFTFT.\n\
        TFTFTFT\n\
        FTFTFTF\n\
        TFTFTFT\n\
        FTFTFTF\n\
        TFTFTFT",
    ];

    let mut game = Connect4Game::new(0, ("Alice".to_string(), "Bob".to_string()));
    for board in positive_test_cases {
        game.board_from_str(board);
        assert!(game.check_draw(), "Positive case failed");
    }
    for board in negative_test_cases {
        game.board_from_str(board);
        assert!(!game.check_draw(), "Negative case failed");
    }
}

#[test]
fn test_victory() {
    #[rustfmt::skip]
    let test_cases = vec![
        (
            "horizontal_positive_T",
           ".......\n\
            .......\n\
            .......\n\
            .......\n\
            ...TTTT\n\
            .......\n",
            Some(true)
        ),
        (
            "horizontal_positive_F",
           ".......\n\
            .......\n\
            .FFFF..\n\
            .......\n\
            .......\n\
            .......\n",
            Some(false)
        ),
        (
            "horizontal_negative",
           ".......\n\
            ...FFF.\n\
            .......\n\
            .TTTFF.\n\
            ...FFFT.\n\
            .......\n",
            None
        ),
        (
            "vertical_positive_T",
           ".......\n\
            ....T..\n\
            ....T..\n\
            ....T..\n\
            ...TTFT\n\
            .......\n",
            Some(true)
        ),
        (
            "vertical_positive_F",
           "F......\n\
            F......\n\
            FTFFF..\n\
            F......\n\
            F......\n\
            .......\n",
            Some(false)
        ),
        (
            "vertical_negative",
           ".......\n\
            ...FFF.\n\
            ...FT..\n\
            .TTTFF.\n\
            ...FFFT\n\
            .....F.\n",
            None
        ),
        (
            "diag_slope_up_positive_T",
           ".......\n\
            ......T\n\
            .....T.\n\
            ....T..\n\
            ...TFTT\n\
            .......\n",
            Some(true)
        ),
        (
            "diag_slope_up_positive_F",
           ".......\n\
            .......\n\
            .FFFTTF\n\
            .....FT\n\
            ....FTF\n\
            ...FTTT\n",
            Some(false)
        ),
        (
            "diag_slope_up_negative",
           ".......\n\
            ...FFF.\n\
            ...T...\n\
            .TTTFF.\n\
            .T.FFFT.\n\
            .......\n",
            None
        ),
        (
            "diag_slope_down_positive_T",
           ".......\n\
            ..T...T\n\
            ...T.F.\n\
            ....T..\n\
            ...TFTT\n\
            .......\n",
            Some(true)
        ),
        (
            "diag_slope_down_positive_F",
           ".F.....\n\
            ..F....\n\
            .FFFTTF\n\
            ....FFT\n\
            ....TTF\n\
            ...FTTT\n",
            Some(false)
        ),
        (
            "diag_slope_down_negative",
           "F......\n\
            .F.FFF.\n\
            T.FT.T.\n\
            .TTTFF.\n\
            .TTFFFT.\n\
            .FF...F\n",
            None
        )
    ];

    let mut game = Connect4Game::new(0, ("Alice".to_string(), "Bob".to_string()));
    for (test_title, board, winner) in test_cases {
        game.board_from_str(board);
        assert_eq!(
            game.check_victory(),
            winner,
            "Test case failed: {}",
            test_title
        );
    }
}
