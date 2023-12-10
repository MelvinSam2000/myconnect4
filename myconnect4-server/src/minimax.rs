use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;

use crate::game::Connect4Game;
use crate::game::COLS;

const SEARCH_TREE_MAX_DEPTH: usize = 6;

pub fn best_move(game: &mut Connect4Game) -> Option<u8> {
    if game.is_gameover().is_some() {
        return None;
    }

    let scores: Vec<(i64, u8)> = (0..COLS)
        .into_par_iter()
        .flat_map(|col| {
            let col = col as u8;
            let mut game = game.clone();
            if !game.play_no_user(col) {
                return None;
            }
            let score = minscore(&mut game, SEARCH_TREE_MAX_DEPTH);
            Some((score, col))
        })
        .collect();

    let max_score = scores
        .iter()
        .map(|x| x.0)
        .fold(i64::MIN, std::cmp::Ord::max);

    let possible_moves = scores
        .iter()
        .filter(|x| x.0 == max_score)
        .map(|x| x.1)
        .collect::<Vec<_>>();

    let i = rand::random::<usize>() % possible_moves.len();
    let col = possible_moves[i];
    if !game.play_no_user(col) {
        log::error!("Could not play the move suggested by minimax");
    }
    Some(col)
}

fn minscore(game: &mut Connect4Game, depth: usize) -> i64 {
    if let Some(game_over) = game.is_gameover() {
        return match game_over {
            crate::game::GameOver::Winner(_) => 1 + depth as i64,
            crate::game::GameOver::Draw => 0,
        };
    }

    if depth == 0 {
        return 0;
    }

    let mut best_score = i64::MAX;

    for col in 0..COLS {
        let col = col as u8;
        if !game.play_no_user(col) {
            continue;
        }
        let score = maxscore(game, depth - 1);
        if score < best_score {
            best_score = score;
        }
        if !game.undo_play(col) {
            log::error!("minscore undo play incorrectly");
        }
    }
    best_score
}

fn maxscore(game: &mut Connect4Game, depth: usize) -> i64 {
    if let Some(game_over) = game.is_gameover() {
        return match game_over {
            crate::game::GameOver::Winner(_) => -1 * (1 + depth as i64),
            crate::game::GameOver::Draw => 0,
        };
    }

    if depth == 0 {
        return 0;
    }

    let mut best_score = i64::MIN;

    for col in 0..COLS {
        let col = col as u8;
        if !game.play_no_user(col) {
            continue;
        }
        let score = minscore(game, depth - 1);
        if score > best_score {
            best_score = score;
        }
        if !game.undo_play(col) {
            log::error!("maxscore undo play incorrectly");
        }
    }
    best_score
}
