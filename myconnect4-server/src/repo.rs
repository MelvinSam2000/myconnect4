use std::collections::HashMap;
use std::collections::HashSet;

use crate::game::Connect4Game;

/*

RELATIONAL DIAGRAM:
game 0..1 --- 2 user

*/

#[derive(Default, Debug)]
pub struct Connect4Repo {
    map_user_to_game_id: HashMap<String, u64>,
    map_game_id_to_users: HashMap<u64, [String; 2]>,
    map_game_id_to_game: HashMap<u64, Connect4Game>,
    set_users: HashSet<String>,
}

impl Connect4Repo {
    pub fn create_new_game(&mut self, users: [String; 2]) -> u64 {
        let game_id = rand::random::<u64>();
        self.map_game_id_to_game
            .insert(game_id, Connect4Game::new(users.clone()));
        self.map_game_id_to_users.insert(game_id, users.clone());
        self.map_user_to_game_id.insert(users[0].clone(), game_id);
        self.map_user_to_game_id.insert(users[1].clone(), game_id);
        game_id
    }

    pub fn get_game_mut(&mut self, game_id: u64) -> Option<&mut Connect4Game> {
        self.map_game_id_to_game.get_mut(&game_id)
    }

    pub fn get_game_id(&self, user: &str) -> Option<u64> {
        self.map_user_to_game_id.get(user).cloned()
    }

    pub fn delete_game(&mut self, game_id: u64) {
        self.map_game_id_to_game.remove(&game_id);
        if let Some(users) = self.map_game_id_to_users.remove(&game_id) {
            self.map_user_to_game_id.remove(&users[0]);
            self.map_user_to_game_id.remove(&users[1]);
        }
    }

    pub fn create_user(&mut self, user: &str) {
        if !self.validate_user(user) {
            return;
        }
        self.set_users.insert(user.to_string());
    }

    pub fn validate_user(&self, user: &str) -> bool {
        if !((3..10).contains(&user.len())) {
            return false;
        }
        if self.set_users.contains(user) {
            return false;
        }
        true
    }

    pub fn delete_user(&mut self, user: &str) -> Option<String> {
        self.set_users.remove(user);
        if let Some(game_id) = self.map_user_to_game_id.remove(user) {
            self.map_game_id_to_game.remove(&game_id);
            let users = self
                .map_game_id_to_users
                .remove(&game_id)
                .expect("GameID for this user should exist... MAJOR BUG");
            let rival = if user == users[0] {
                users[1].clone()
            } else {
                users[0].clone()
            };
            self.map_user_to_game_id.remove(&rival);
            return Some(rival);
        }
        None
    }
}
