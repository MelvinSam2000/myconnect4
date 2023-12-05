pub mod myconnect4 {
    tonic::include_proto!("myconnect4");
}
pub mod actor;
pub mod game;
pub mod repo;

use env_logger::Builder;
use env_logger::Env;
use opentelemetry::global;

use crate::actor::controller::ActorController;

fn init_logs() {
    Builder::from_env(Env::default())
        .format_timestamp_nanos()
        .init();
}

fn init_tracing() {
    let provider = opentelemetry_jaeger::new_agent_pipeline()
        .with_service_name("Connect 4 server")
        .with_endpoint("127.0.0.1:6831")
        .build_simple()
        .unwrap();
    global::set_tracer_provider(provider);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logs();
    init_tracing();
    let actor_controller = ActorController::new();
    actor_controller.run_all().await;
    Ok(())
}

#[cfg(test)]
pub mod tests;
