pub struct Config {
    pub(crate) listener_address: String,
    pub(crate) listener_port: u16
}

const LISTENER_ADDRESS_ENV: &str = "LISTENER_ADDRESS";
const LISTENER_PORT_ENV: &str = "LISTENER_PORT";

#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("missing env var: {0}")]
    MissingVar(&'static str),
    #[error("invalid port: {0}")]
    InvalidPort(#[from] std::num::ParseIntError),
}

impl Config {
    pub fn new () -> Result<Config, ConfigError> {
        let listener_address = std::env::var(LISTENER_ADDRESS_ENV)
            .map_err( |_| ConfigError::MissingVar(LISTENER_ADDRESS_ENV))?;

        let listener_port = std::env::var(LISTENER_PORT_ENV)
            .map_err( |_| ConfigError::MissingVar(LISTENER_PORT_ENV))?
            .parse::<u16>()?;

        Ok(Config{
            listener_address,
            listener_port
        })
    }
}