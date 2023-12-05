pub mod myconnect4 {
    tonic::include_proto!("myconnect4");
}
pub mod actor;
pub mod game;
pub mod repo;

use env_logger::Builder;
use env_logger::Env;

use crate::actor::controller::ActorController;

fn init_logs() {
    Builder::from_env(Env::default())
        .format_timestamp_nanos()
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logs();
    let actor_controller = ActorController::new();
    actor_controller.run_all().await;
    Ok(())
}

#[cfg(test)]
pub mod tests;
