pub mod myconnect4 {
    tonic::include_proto!("myconnect4");
}
pub mod actor;
pub mod game;
pub mod minimax;
pub mod repo;

const NUM_TOKIO_CPU_CORES: usize = 2;
const NUM_RAYON_CPU_CORES: usize = 6;

use env_logger::Builder;
use env_logger::Env;

use crate::actor::controller::ActorController;

fn init_logs() {
    Builder::from_env(Env::default())
        .format_timestamp_nanos()
        .init();
}

fn main() {
    let main_task = async {
        init_logs();
        let actor_controller = ActorController::new();
        actor_controller.run_all().await;
    };
    rayon::ThreadPoolBuilder::new()
        .num_threads(NUM_RAYON_CPU_CORES)
        .build_global()
        .expect("Could not start rayon");
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(NUM_TOKIO_CPU_CORES)
        .build()
        .expect("Could not start tokio runtime")
        .block_on(main_task);
}

#[cfg(test)]
pub mod tests;
