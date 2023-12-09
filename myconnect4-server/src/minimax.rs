use crate::game::Connect4Game;
use crate::game::COLS;

pub fn best_move(game: &mut Connect4Game) -> Option<u8> {
    let mut valid = false;
    let mut col = 0;
    for _ in 0..100 {
        col = rand::random::<u8>() % COLS as u8;
        if game.play_no_user(col) {
            valid = true;
            break;
        }
    }
    let bot_move = if valid { Some(col) } else { None };
    bot_move
}
