use std::env;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use thiserror::Error;

const SERVER_ADDRESS: &str = "SERVER_ADDRESS";
const SERVER_PORT: &str = "SERVER_PORT";
const FSYNC_POLICY: &str = "FSYNC_POLICY";
const AOF_PATH: &str = "AOF_PATH";
const FSYNC_POLICY_NAMES: [&str; 4] = ["ONE_MIN", "TWO_MIN", "THREE_MIN", "FIVE_MIN"];

pub struct Config {
    pub server_address: String,
    pub server_port: u16,
    pub fsync_policy: FsyncPolicy,
    pub aof_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsyncPolicy {
    OneMin,
    TwoMin,
    ThreeMin,
    FiveMin,
}

#[derive(Error, Debug, PartialEq)]
#[error(
    "unrecognized fsync policy: {0}, expected one of: {names:?}",
    names = FSYNC_POLICY_NAMES
)]
pub struct ParseFsyncPolicyError(String);

impl FromStr for FsyncPolicy {
    type Err = ParseFsyncPolicyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ONE_MIN" => Ok(Self::OneMin),
            "TWO_MIN" => Ok(Self::TwoMin),
            "THREE_MIN" => Ok(Self::ThreeMin),
            "FIVE_MIN" => Ok(Self::FiveMin),
            other => Err(ParseFsyncPolicyError(other.to_owned())),
        }
    }
}

impl FsyncPolicy {
    pub fn duration(self) -> Duration {
        match self {
            Self::OneMin => Duration::from_secs(60),
            Self::TwoMin => Duration::from_secs(120),
            Self::ThreeMin => Duration::from_secs(180),
            Self::FiveMin => Duration::from_secs(300),
        }
    }
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
        let fsync_policy = env::var(FSYNC_POLICY)
            .unwrap_or_else(|_| panic!("{FSYNC_POLICY} must be set"))
            .parse()
            .unwrap_or_else(|_| panic!("{FSYNC_POLICY} must be one of {FSYNC_POLICY_NAMES:?}"));
        let aof_path = env::var(AOF_PATH).ok().map(PathBuf::from);

        Config {
            server_address,
            server_port,
            fsync_policy,
            aof_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fsync_policy_from_env_names() {
        assert_eq!("ONE_MIN".parse(), Ok(FsyncPolicy::OneMin));
        assert_eq!("TWO_MIN".parse(), Ok(FsyncPolicy::TwoMin));
        assert_eq!("THREE_MIN".parse(), Ok(FsyncPolicy::ThreeMin));
        assert_eq!("FIVE_MIN".parse(), Ok(FsyncPolicy::FiveMin));
    }

    #[test]
    fn rejects_unknown_fsync_policy() {
        let err = "ALWAYS"
            .parse::<FsyncPolicy>()
            .expect_err("unknown policy should fail");
        assert!(matches!(err, ParseFsyncPolicyError(name) if name == "ALWAYS"));
    }

    #[test]
    fn duration_matches_policy_interval() {
        assert_eq!(FsyncPolicy::OneMin.duration(), Duration::from_secs(60));
        assert_eq!(FsyncPolicy::TwoMin.duration(), Duration::from_secs(120));
        assert_eq!(FsyncPolicy::ThreeMin.duration(), Duration::from_secs(180));
        assert_eq!(FsyncPolicy::FiveMin.duration(), Duration::from_secs(300));
    }
}
