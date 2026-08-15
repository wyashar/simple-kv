use std::env;

const SERVER_ADDRESS: &str = "SERVER_ADDRESS";
const SERVER_PORT: &str = "SERVER_PORT";

pub struct Config {
    pub server_address: String,
    pub server_port: u16,
}

impl Config {
    pub fn from_env() -> Config {
        dotenvy::dotenv().ok();

        let server_address = env::var(SERVER_ADDRESS).expect("SERVER_ADDRESS must be set");
        let server_port = env::var(SERVER_PORT)
            .expect("SERVER_PORT must be set")
            .parse()
            .expect("SERVER_PORT must be a valid u16");

        Config {
            server_address,
            server_port,
        }
    }
}
