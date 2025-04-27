use log::error;

#[tokio::main]
async fn main() -> Result<(), String> {
    let env = env_logger::Env::default().filter_or("RUST_LOG", "info");

    env_logger::init_from_env(env);

    let args = std::env::args().collect::<Vec<_>>();
    match &args[1..] {
        [command, config_file] if command == "--test" => {
            ifdyndnsd::config::load(config_file).unwrap();
            Ok(())
        }
        [config_file] => {
            ifdyndnsd::run(config_file).await.unwrap();
            panic!("ifdyndnsd exited");
        }
        _ => {
            error!("Usage: {} [--test] <config.toml>", args[0]);
            std::process::exit(1);
        }
    }
}
