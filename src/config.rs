use strum::VariantNames;

use crate::append_only_file::FsyncPolcy;

pub struct Config {
    pub server_address: String,
    pub server_port: u16,
    pub fsync_policy: FsyncPolcy,
}

const SERVER_ADDRESS_ENV: &'static str = "SERVER_ADDRESS";
const SERVER_PORT_ENV: &'static str = "SERVER_PORT";
const FSYNC_POLICY: &'static str = "FSYNC_POLICY";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{env_var} must be provided", env_var = SERVER_ADDRESS_ENV)]
    MissingServerAddress,
    #[error("{env_var} must be provided", env_var = SERVER_PORT_ENV)]
    MissingServerPort,
    #[error("{env_var} must be a valid u16 {0:?}", env_var = SERVER_PORT_ENV)]
    InvalidPortFormat(#[from] std::num::ParseIntError),
    #[error("{env_var} must be provided", env_var = FSYNC_POLICY)]
    MissingFsyncPolicy,
    #[error("{env_var} must be one of {variant_names:?}", env_var = FSYNC_POLICY, variant_names = FsyncPolcy::VARIANTS)]
    InvalidFsyncPolicy,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let server_address =
            std::env::var(SERVER_ADDRESS_ENV).map_err(|_| ConfigError::MissingServerAddress)?;

        let server_port: u16 = std::env::var(SERVER_PORT_ENV)
            .map_err(|_| ConfigError::MissingServerPort)?
            .parse::<u16>()?;

        let fsync_policy: FsyncPolcy = std::env::var(FSYNC_POLICY)
            .map_err(|_| ConfigError::MissingFsyncPolicy)?
            .parse()
            .map_err(|_| ConfigError::InvalidFsyncPolicy)?;

        Ok(Self {
            server_address,
            server_port,
            fsync_policy,
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
            std::env::remove_var(FSYNC_POLICY);
        }
    }

    #[test]
    #[serial]
    fn from_env_succeeds_with_valid_input() {
        reset_env();
        unsafe {
            std::env::set_var(SERVER_ADDRESS_ENV, "127.0.0.1");
            std::env::set_var(SERVER_PORT_ENV, "8080");
            std::env::set_var(FSYNC_POLICY, "EverySec");
        }

        let config = Config::from_env().unwrap();
        assert_eq!(config.server_address, "127.0.0.1");
        assert_eq!(config.server_port, 8080);
        assert_eq!(config.fsync_policy, FsyncPolcy::EverySec);
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
        assert!(matches!(result, Err(ConfigError::InvalidPortFormat(_))));
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
        assert!(matches!(result, Err(ConfigError::InvalidPortFormat(_))));
    }
}
