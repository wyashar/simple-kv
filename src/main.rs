use crate::tcp_handler::ListenError;
use crate::config::{Config};

mod tcp_handler;
mod config;

fn main() {
    dotenvy::dotenv().ok();
    
    let config: Config = Config::new().unwrap_or_else(|e| panic!("{}", e));

    match tcp_handler::listen_on(&format!("{}:{}", config.listener_address, config.listener_port)) {
        Err(e) => match e {
            ListenError::AddrParse(e) => panic!("Failed to parse listening addr: {:?}", e),
            ListenError::Io(e) => panic!("Couldn't listen on: {:?}", e)
        },
        _ => (),
    };
}