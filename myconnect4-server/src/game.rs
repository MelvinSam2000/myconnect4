pub(crate) const ROWS: usize = 6;
pub(crate) const COLS: usize = 7;

#[derive(Debug, Clone)]
pub enum GameOver {
    Winner(String),
    Draw,
}

#[derive(Debug, Clone, Default)]
pub struct Connect4Game {
    board: [[Option<bool>; COLS]; ROWS],
    pub users: (String, String),
    pub user_first: String,
    turn: bool,
    pub game_id: u64,
    history: Vec<u8>,
}

impl Connect4Game {
    pub fn new(game_id: u64, users: (String, String)) -> Self {
        let user_first = if rand::random::<bool>() {
            users.0.clone()
        } else {
            users.1.clone()
        };
        Self {
            board: [[None; COLS]; ROWS],
            users: users.clone(),
            user_first: user_first.clone(),
            turn: user_first == users.0,
            game_id,
            history: vec![],
        }
    }

    pub fn new_custom(game_id: u64, player: String, rival: String, is_first: bool) -> Self {
        let (first, second) = if is_first {
            (player, rival)
        } else {
            (rival, player)
        };
        Self {
            board: [[None; COLS]; ROWS],
            users: (first.clone(), second),
            user_first: first,
            turn: true,
            game_id,
            history: vec![],
        }
    }

    pub fn get_rival(&self, user: &str) -> String {
        if user == &self.users.0 {
            self.users.1.clone()
        } else {
            self.users.0.clone()
        }
    }

    pub fn play_no_user(&mut self, col: u8) -> bool {
        let user = if self.turn {
            self.users.0.clone()
        } else {
            self.users.1.clone()
        };
        self.play(&user, col)
    }

    pub fn play(&mut self, user: &str, col: u8) -> bool {
        let col = col as usize;
        if col >= COLS {
            log::warn!("User {user} made invalid column for move: {col}");
            return false;
        }
        let player = self.users.0 == user;
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

        if row >= ROWS {
            return false;
        }
        self.board[row][col] = Some(player);
        self.history.push(col as u8);

        self.turn = !self.turn;

        true
    }

    pub fn undo_play(&mut self, col: u8) -> bool {
        let col = col as usize;
        if col >= COLS {
            log::warn!("Undo play for invalid column: {col}");
            return false;
        }
        let mut found = false;
        for row in (0..ROWS).rev() {
            if self.board[row][col].is_some() {
                self.board[row][col] = None;
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
        self.history.pop();
        self.turn = !self.turn;
        true
    }

    pub fn is_gameover(&self) -> Option<GameOver> {
        if let Some(user0_wins) = self.check_victory() {
            return Some(GameOver::Winner(if user0_wins {
                self.users.0.clone()
            } else {
                self.users.1.clone()
            }));
        }

        if self.check_draw() {
            return Some(GameOver::Draw);
        }

        None
    }

    pub fn check_draw(&self) -> bool {
        self.board[ROWS - 1].iter().all(Option::is_some)
    }

    pub fn check_victory(&self) -> Option<bool> {
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
