use crate::myconnect4::game_over::Kind;
use crate::myconnect4::Empty;
use crate::myconnect4::GameOver;
use crate::myconnect4::Winner;

const ROWS: usize = 6;
const COLS: usize = 7;

#[derive(Debug)]
pub struct Connect4Game {
    board: [[Option<bool>; COLS]; ROWS],
    pub users: [String; 2],
    pub user_first: String,
    turn: bool,
}

impl Connect4Game {
    pub fn new(users: [String; 2]) -> Self {
        let user_first = if rand::random::<bool>() {
            users[0].clone()
        } else {
            users[1].clone()
        };
        Self {
            board: [[None; 7]; 6],
            users: users.clone(),
            user_first: user_first.clone(),
            turn: user_first == users[0],
        }
    }

    pub fn play(&mut self, user: &str, col: usize) -> bool {
        if col >= COLS {
            log::warn!("Invalid column for move: {col}");
            return false;
        }
        let player = self.users[0] == user;
        if self.turn != player {
            return false;
        }
        let mut row = ROWS - 1;
        while self.board[row][col].is_none() && row > 0 {
            row -= 1;
        }
        if self.board[row][col].is_some() {
            row += 1;
        }

        if row > ROWS {
            return false;
        }
        self.board[row][col] = Some(player);

        self.turn = !self.turn;

        true
    }

    pub fn is_gameover(&self) -> Option<GameOver> {
        if self.check_draw() {
            return Some(GameOver {
                kind: Some(Kind::Draw(Empty {})),
            });
        }

        if let Some(user0_wins) = self.check_victory() {
            return Some(GameOver {
                kind: Some(Kind::Winner(Winner {
                    user: if user0_wins {
                        self.users[0].clone()
                    } else {
                        self.users[1].clone()
                    },
                })),
            });
        }

        None
    }

    fn check_draw(&self) -> bool {
        self.board[ROWS - 1].iter().all(Option::is_some)
    }

    fn check_victory(&self) -> Option<bool> {
        // horizontal
        for i in 0..ROWS {
            for j in 0..=COLS - 4 {
                let winner = (0..4)
                    .map(|k| self.board[i][j + k])
                    .reduce(|x, y| if x == y { x } else { None })
                    .flatten();
                if winner.is_some() {
                    return winner;
                }
            }
        }
        // vertical
        for i in 0..=ROWS - 4 {
            for j in 0..COLS {
                let winner = (0..4)
                    .map(|k| self.board[i + k][j])
                    .reduce(|x, y| if x == y { x } else { None })
                    .flatten();
                if winner.is_some() {
                    return winner;
                }
            }
        }
        // diagonal slope up
        for i in 0..=ROWS - 4 {
            for j in 0..=COLS - 4 {
                let winner = (0..4)
                    .map(|k| self.board[i + k][j + k])
                    .reduce(|x, y| if x == y { x } else { None })
                    .flatten();
                if winner.is_some() {
                    return winner;
                }
            }
        }
        // diagonal slope down
        for i in 3..ROWS {
            for j in 0..=COLS - 4 {
                let winner = (0..4)
                    .map(|k| self.board[i - k][j + k])
                    .reduce(|x, y| if x == y { x } else { None })
                    .flatten();
                if winner.is_some() {
                    return winner;
                }
            }
        }
        None
    }

    pub fn board_from_str(&mut self, board: &str) {
        let board: Vec<Vec<Option<bool>>> = board
            .lines()
            .map(|line| {
                line.chars()
                    .map(|ch| match ch {
                        '.' => None,
                        'T' => Some(true),
                        'F' => Some(false),
                        _ => panic!("Invalid character in board String: {ch}"),
                    })
                    .collect()
            })
            .rev()
            .collect();
        for i in 0..ROWS {
            for j in 0..COLS {
                self.board[i][j] = board[i][j];
            }
        }
    }

    pub fn board_to_str(&self) -> String {
        let mut board_str = [[' '; COLS]; ROWS];
        for i in 0..ROWS {
            for j in 0..COLS {
                if let Some(tile) = self.board[i][j] {
                    board_str[i][j] = if tile { 'T' } else { 'F' };
                }
            }
        }
        let mut result = String::new();
        for row in self.board.iter().rev() {
            for &tile in row {
                let ch = match tile {
                    None => '.',
                    Some(false) => 'F',
                    Some(true) => 'T',
                };
                result.push(ch);
            }
            result.push('\n');
        }
        result
    }
}

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

    let mut game = Connect4Game::new(["Alice".to_string(), "Bob".to_string()]);
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

    let mut game = Connect4Game::new(["Alice".to_string(), "Bob".to_string()]);
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
