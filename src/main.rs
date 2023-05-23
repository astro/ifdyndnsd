use log::error;

#[tokio::main]
async fn main() -> Result<(), String> {
    env_logger::init();

    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 2 {
        error!("Usage: {} <config.toml>", args[0]);
        std::process::exit(1);
    }
    let config_file = &args[1];
    ifdyndnsd::run(config_file).await
}
