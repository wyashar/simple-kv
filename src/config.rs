use std::fmt;

pub struct Config {
    pub server_address: String,
    pub server_port: u16,
}

const SERVER_ADDRESS_ENV: &str = "SERVER_ADDRESS";
const SERVER_PORT_ENV: &str = "SERVER_PORT";

#[derive(Debug)]
pub enum ConfigError {
    MissingServerAddress,
    MissingServerPort,
    InvalidPortFormat,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::MissingServerAddress => write!(
                f,
                "Failed to create new value for Config: {SERVER_ADDRESS_ENV} must be provided by env!"
            ),
            Self::MissingServerPort => write!(
                f,
                "Failed to create new value for Config: {SERVER_PORT_ENV} must be provided by env!"
            ),
            Self::InvalidPortFormat => write!(
                f,
                "Failed to create new value for Config: {SERVER_PORT_ENV} must be a valid u16!"
            ),
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let server_address =
            std::env::var(SERVER_ADDRESS_ENV).map_err(|_| ConfigError::MissingServerAddress)?;

        let server_port = std::env::var(SERVER_PORT_ENV)
            .map_err(|_| ConfigError::MissingServerPort)?
            .parse::<u16>()
            .map_err(|_| ConfigError::InvalidPortFormat)?;

        Ok(Self {
            server_address,
            server_port,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn reset_env() {
        unsafe {
            std::env::remove_var(SERVER_ADDRESS_ENV);
            std::env::remove_var(SERVER_PORT_ENV);
        }
    }

    #[test]
    #[serial]
    fn from_env_succeeds_with_valid_input() {
        reset_env();
        unsafe {
            std::env::set_var(SERVER_ADDRESS_ENV, "127.0.0.1");
            std::env::set_var(SERVER_PORT_ENV, "8080");
        }

        let config = Config::from_env().unwrap();
        assert_eq!(config.server_address, "127.0.0.1");
        assert_eq!(config.server_port, 8080);
    }

    #[test]
    #[serial]
    fn from_env_fails_when_no_server_address() {
        reset_env();
        unsafe {
            std::env::set_var(SERVER_PORT_ENV, "8080");
        }

        let result = Config::from_env();
        assert!(matches!(result, Err(ConfigError::MissingServerAddress)));
    }

    #[test]
    #[serial]
    fn from_env_fails_when_no_server_port() {
        reset_env();
        unsafe {
            std::env::set_var(SERVER_ADDRESS_ENV, "127.0.0.1");
        }

        let result = Config::from_env();
        assert!(matches!(result, Err(ConfigError::MissingServerPort)));
    }

    #[test]
    #[serial]
    fn from_env_fails_when_port_is_not_numeric() {
        reset_env();
        unsafe {
            std::env::set_var(SERVER_ADDRESS_ENV, "127.0.0.1");
            std::env::set_var(SERVER_PORT_ENV, "not-a-number");
        }

        let result = Config::from_env();
        assert!(matches!(result, Err(ConfigError::InvalidPortFormat)));
    }

    #[test]
    #[serial]
    fn from_env_fails_when_port_overflows_u16() {
        reset_env();
        unsafe {
            std::env::set_var(SERVER_ADDRESS_ENV, "127.0.0.1");
            std::env::set_var(SERVER_PORT_ENV, "70000");
        }

        let result = Config::from_env();
        assert!(matches!(result, Err(ConfigError::InvalidPortFormat)));
    }
}
