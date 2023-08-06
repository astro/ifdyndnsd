use log::error;

#[tokio::main]
async fn main() -> Result<(), String> {
    let env = env_logger::Env::default().filter_or("RUST_LOG", "info");

    env_logger::init_from_env(env);

    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 2 {
        error!("Usage: {} <config.toml>", args[0]);
        std::process::exit(1);
    }
    let config_file = &args[1];
    ifdyndnsd::run(config_file).await
}
