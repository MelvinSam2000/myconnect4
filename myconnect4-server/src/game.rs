pub struct Connect4Game {
    board: [[bool; 7]; 6],
    p1_turn: bool,
}

impl Connect4Game {
    pub fn new() -> Self {
        Self {
            board: [[false; 7]; 6],
            p1_turn: true,
        }
    }

    pub fn play(&self, column: usize) {
        todo!()
    }
}
