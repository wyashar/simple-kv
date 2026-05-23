pub struct Config {
    pub listener_address: String,
    pub listener_port: u16,
}

const LISTENER_ADDRESS_ENV: &str = "LISTENER_ADDRESS";
const LISTENER_PORT_ENV: &str = "LISTENER_PORT";

#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("missing listener address")]
    MissingListenerAddress,
    #[error("missing listener port")]
    MissingListenerPort,
    #[error("invalid port: {0}")]
    InvalidPort(#[from] std::num::ParseIntError),
    #[error("port must be a valid u16")]
    InvalidPortFormat,
}

impl Config {
    pub fn new() -> Result<Self, ConfigError> {
        let listener_address =
            std::env::var(LISTENER_ADDRESS_ENV).map_err(|_| ConfigError::MissingListenerAddress)?;

        let listener_port = std::env::var(LISTENER_PORT_ENV)
            .map_err(|_| ConfigError::MissingListenerPort)?
            .parse::<u16>()
            .map_err(|_| ConfigError::InvalidPortFormat)?;

        Ok(Config {
            listener_address,
            listener_port,
        })
    }
}
