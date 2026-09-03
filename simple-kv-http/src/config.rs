use std::env;

const SERVER_ADDRESS: &str = "SERVER_ADDRESS";
const SERVER_PORT: &str = "SERVER_PORT";
const KV_SERVER_ADDRESS: &str = "KV_SERVER_ADDRESS";
const KV_SERVER_PORT: &str = "KV_SERVER_PORT";

pub struct Config {
    pub server_address: String,
    pub server_port: u16,
    pub kv_server_address: String,
    pub kv_server_port: u16,
}

impl Config {
    pub fn from_env() -> Config {
        dotenvy::dotenv().ok();

        let server_address =
            env::var(SERVER_ADDRESS).unwrap_or_else(|_| panic!("{SERVER_ADDRESS} must be set"));
        let server_port = env::var(SERVER_PORT)
            .unwrap_or_else(|_| panic!("{SERVER_PORT} must be set"))
            .parse()
            .unwrap_or_else(|_| panic!("{SERVER_PORT} must be a valid u16"));
        let kv_server_address = env::var(KV_SERVER_ADDRESS)
            .unwrap_or_else(|_| panic!("{KV_SERVER_ADDRESS} must be set"));
        let kv_server_port = env::var(KV_SERVER_PORT)
            .unwrap_or_else(|_| panic!("{KV_SERVER_PORT} must be set"))
            .parse()
            .unwrap_or_else(|_| panic!("{KV_SERVER_PORT} must be a valid u16"));

        Config {
            server_address,
            server_port,
            kv_server_address,
            kv_server_port,
        }
    }
}
