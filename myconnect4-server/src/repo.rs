use std::collections::HashMap;

use crate::game::Connect4Game;

#[derive(Default)]
pub struct Connect4Repo {
    map_user_to_game_id: HashMap<String, u64>,
    map_game_id_to_users: HashMap<u64, [String; 2]>,
    map_game_id_to_game: HashMap<u64, Connect4Game>,
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
}
