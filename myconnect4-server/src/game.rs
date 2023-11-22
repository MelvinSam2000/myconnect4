use crate::myconnect4::game_over::Kind;
use crate::myconnect4::Empty;
use crate::myconnect4::GameOver;

pub struct Connect4Game {
    board: [[Option<bool>; 7]; 6],
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

    pub fn play(&self, column: u8) -> bool {
        // TODO
        true
    }

    pub fn is_gameover(&self) -> Option<GameOver> {
        // TODO
        if rand::random::<bool>() && rand::random::<bool>() {
            Some(GameOver {
                kind: Some(Kind::Draw(Empty {})),
            })
        } else {
            None
        }
    }
}
