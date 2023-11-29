pub mod myconnect4 {
    tonic::include_proto!("myconnect4");
}
pub mod actor;
pub mod game;
pub mod repo;

use crate::actor::controller::ActorController;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let actor_controller = ActorController::new();
    actor_controller.run_all().await;
    Ok(())
}

#[cfg(test)]
pub mod tests;
