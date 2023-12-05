use std::time::Duration;

pub const BUFFER_MAX: usize = 100000;
pub const HB_SEND_DUR: Duration = Duration::from_secs(5);
pub const HB_RECV_WAIT_LIMIT: Duration = Duration::from_secs(10);

pub mod bot;
pub mod botmanager;
pub mod controller;
pub mod game;
pub mod matchmaking;
pub mod service;
