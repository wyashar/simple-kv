use std::env;
use std::path::PathBuf;
use std::time::Duration;

const SERVER_ADDRESS: &str = "SERVER_ADDRESS";
const SERVER_PORT: &str = "SERVER_PORT";
const SYNC_INTERVAL: &str = "SYNC_INTERVAL";
const WAL_PATH: &str = "WAL_PATH";
const MIN_SYNC_INTERVAL_SECS: u64 = 30;

pub struct Config {
    pub server_address: String,
    pub server_port: u16,
    pub sync_interval: Duration,
    pub wal_path: Option<PathBuf>,
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
        let sync_interval_secs: u64 = env::var(SYNC_INTERVAL)
            .unwrap_or_else(|_| panic!("{SYNC_INTERVAL} must be set"))
            .parse()
            .unwrap_or_else(|_| panic!("{SYNC_INTERVAL} must be a number of seconds"));
        if sync_interval_secs < MIN_SYNC_INTERVAL_SECS {
            panic!("{SYNC_INTERVAL} must be at least {MIN_SYNC_INTERVAL_SECS} seconds");
        }
        let sync_interval = Duration::from_secs(sync_interval_secs);
        let wal_path = env::var(WAL_PATH).ok().map(PathBuf::from);

        Config {
            server_address,
            server_port,
            sync_interval,
            wal_path,
        }
    }
}
