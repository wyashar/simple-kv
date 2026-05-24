use crate::config::Config;
use crate::tcp_server::TcpServer;
use env_logger;
use log::error;

mod config;
mod kv_store;
mod tcp_handler;
mod tcp_server;
mod wire_format;

fn main() {
    dotenvy::dotenv().ok();
    env_logger::init();

    let config: Config = Config::from_env().unwrap_or_else(|e| panic!("{}", e));
    let tcp_server: TcpServer = TcpServer::bind(&config.server_address, config.server_port)
        .unwrap_or_else(|e| {
            error!("Failed to start tcp server: {e}");
            std::process::exit(1);
        });
}
