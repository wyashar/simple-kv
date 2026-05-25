use crate::config::Config;
use env_logger;

mod config;
mod kv_server;
mod kv_store;
mod tcp_server;
mod wire_format;

fn main() {
    dotenvy::dotenv().ok();
    env_logger::init();

    let config: Config = Config::from_env().unwrap_or_else(|e| panic!("{e}"));
    kv_server::run(config);
}
