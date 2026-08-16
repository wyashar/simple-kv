mod command;
mod config;
mod request;
mod response;
mod server;
mod util;

use config::Config;

fn main() {
    env_logger::init();

    let config = Config::from_env();
    let addr = format!("{}:{}", config.server_address, config.server_port);

    server::run(&addr);
}
