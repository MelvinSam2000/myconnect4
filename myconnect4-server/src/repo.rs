use std::collections::HashMap;
use std::collections::VecDeque;

use chrono::DateTime;
use chrono::Utc;

use crate::game::Connect4Game;
use crate::game::GameOver;

/*
RELATIONAL DIAGRAM:
game 0..1 --- 2 user
*/

const PAST_GAMES_LIMIT: usize = 100;

pub type PastGameInfo = (Connect4Game, Option<GameOver>, DateTime<Utc>);

#[derive(Default, Debug)]
pub struct Connect4Repo {
    map_user_to_game_id: HashMap<String, u64>,
    map_game_id_to_users: HashMap<u64, (String, String)>,
    map_game_id_to_game: HashMap<u64, Connect4Game>,
    past_games: VecDeque<PastGameInfo>,
    pub total_games_played: u128,
}

impl Connect4Repo {
    pub fn create_new_game(&mut self, users: (String, String)) -> u64 {
        let game_id = rand::random::<u64>();
        self.map_game_id_to_game
            .insert(game_id, Connect4Game::new(game_id, users.clone()));
        self.map_game_id_to_users.insert(game_id, users.clone());
        self.map_user_to_game_id.insert(users.0.clone(), game_id);
        self.map_user_to_game_id.insert(users.1.clone(), game_id);
        game_id
    }

    pub fn get_game(&mut self, game_id: u64) -> Option<&mut Connect4Game> {
        self.map_game_id_to_game.get_mut(&game_id)
    }

    pub fn get_game_id(&self, user: &str) -> Option<u64> {
        self.map_user_to_game_id.get(user).cloned()
    }

    pub fn get_game_ids(&self) -> Vec<u64> {
        self.map_game_id_to_game.keys().cloned().collect()
    }

    pub fn get_users(&self) -> Vec<String> {
        self.map_user_to_game_id.keys().cloned().collect()
    }

    pub fn get_past_games(&self, num: usize) -> Vec<PastGameInfo> {
        self.past_games.iter().rev().take(num).cloned().collect()
    }

    pub fn delete_game(&mut self, game_id: u64) {
        let game = self.map_game_id_to_game.remove(&game_id).unwrap();
        let gameover = game.is_gameover();
        self.past_games.push_front((game, gameover, Utc::now()));
        if self.past_games.len() >= PAST_GAMES_LIMIT {
            self.past_games.pop_back();
        }
        if let Some(users) = self.map_game_id_to_users.remove(&game_id) {
            self.map_user_to_game_id.remove(&users.0);
            self.map_user_to_game_id.remove(&users.1);
            self.total_games_played += 1;
            if self.total_games_played == u128::MAX {
                self.total_games_played = 0;
                log::info!("Total games played reached reached u128::MAX. Resetting counter.");
            }
        }
    }

    pub fn delete_user(&mut self, user: &str) -> Option<String> {
        if let Some(game_id) = self.map_user_to_game_id.remove(user) {
            self.map_game_id_to_game.remove(&game_id);
            let users = self
                .map_game_id_to_users
                .remove(&game_id)
                .expect("GameID for this user should exist... MAJOR BUG");
            let rival = if user == users.0 {
                users.1.clone()
            } else {
                users.0.clone()
            };
            self.map_user_to_game_id.remove(&rival);
            return Some(rival);
        }
        None
    }
}
