use std::fmt;

use thiserror::Error;

use crate::command::CommandError::{TooFewArguments, TooManyArguments, UnrecognizedCommand};
use crate::request::Request;
use crate::util::Bytes;

const GET_STR: &'static str = "GET";
const SET_STR: &'static str = "SET";
const DEL_STR: &'static str = "DEL";
const COMMAND_NAMES: [&'static str; 3] = [GET_STR, SET_STR, DEL_STR];

#[derive(Debug, PartialEq)]
pub enum Command {
    Get(Bytes),
    Set(Bytes, Bytes),
    Del(Vec<Bytes>),
}

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("command name was not valid utf-8")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    #[error("unrecognized command name: {0}, expected one of: {names:?}", names = COMMAND_NAMES)]
    UnrecognizedCommand(String),
    #[error("request must have at least 2 elements, got {0}")]
    TooFewArguments(usize),
    #[error("request had too many elements: {0}")]
    TooManyArguments(usize),
}

impl Command {
    pub fn name(&self) -> &str {
        match self {
            Self::Del(_) => DEL_STR,
            Self::Set(_, _) => SET_STR,
            Self::Get(_) => GET_STR,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut parts: Vec<&[u8]> = vec![self.name().as_bytes()];
        match self {
            Self::Get(key) => parts.push(key),
            Self::Set(key, value) => {
                parts.push(key);
                parts.push(value);
            }
            Self::Del(keys) => parts.extend(keys.iter().map(Bytes::as_slice)),
        }

        let mut buf = format!("*{}\r\n", parts.len()).into_bytes();
        for part in parts {
            buf.extend_from_slice(format!("${}\r\n", part.len()).as_bytes());
            buf.extend_from_slice(part);
            buf.extend_from_slice(b"\r\n");
        }

        buf
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())?;

        match self {
            Self::Get(key) => write!(f, " {}", String::from_utf8_lossy(key))?,
            Self::Set(key, value) => write!(
                f,
                " {} {}",
                String::from_utf8_lossy(key),
                String::from_utf8_lossy(value)
            )?,
            Self::Del(keys) => {
                for key in keys {
                    write!(f, " {}", String::from_utf8_lossy(key))?;
                }
            }
        }

        Ok(())
    }
}

impl TryFrom<Request> for Command {
    type Error = CommandError;

    fn try_from(request: Request) -> Result<Self, CommandError> {
        let commands = request.into_args();

        if commands.len() < 2 {
            return Err(TooFewArguments(commands.len()));
        }

        let total = commands.len();
        let mut args = commands.into_iter();
        let name_bytes = args.next().expect("len >= 2 checked above");
        let command_name = str::from_utf8(&name_bytes)?;

        match command_name {
            GET_STR => {
                if args.len() > 1 {
                    return Err(TooManyArguments(total));
                }
                Ok(Self::Get(args.next().expect("len >= 2 checked above")))
            }
            SET_STR => {
                if args.len() < 2 {
                    return Err(TooFewArguments(total));
                }
                if args.len() > 2 {
                    return Err(TooManyArguments(total));
                }

                Ok(Self::Set(
                    args.next().expect("len >= 2 checked above"),
                    args.next().expect("len >= 2 checked above"),
                ))
            }
            DEL_STR => Ok(Self::Del(args.collect())),
            other => Err(UnrecognizedCommand(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_of(parts: &[&[u8]]) -> Request {
        Request::from_args(parts.iter().map(|p| p.to_vec()).collect())
    }

    fn parse(parts: &[&[u8]]) -> Result<Command, CommandError> {
        Command::try_from(request_of(parts))
    }

    #[test]
    fn parses_get() {
        assert_eq!(
            parse(&[b"GET", b"mykey"]).unwrap(),
            Command::Get(b"mykey".to_vec())
        );
    }

    #[test]
    fn parses_set() {
        assert_eq!(
            parse(&[b"SET", b"mykey", b"myval"]).unwrap(),
            Command::Set(b"mykey".to_vec(), b"myval".to_vec()),
        );
    }

    #[test]
    fn parses_del_multiple_keys() {
        assert_eq!(
            parse(&[b"DEL", b"k1", b"k2"]).unwrap(),
            Command::Del(vec![b"k1".to_vec(), b"k2".to_vec()]),
        );
    }

    #[test]
    fn keys_and_values_need_not_be_utf8() {
        assert_eq!(
            parse(&[b"GET", b"\xff\xfe\x00"]).unwrap(),
            Command::Get(vec![0xff, 0xfe, 0x00]),
        );
    }

    #[test]
    fn too_few_args_when_under_two() {
        let err = parse(&[b"GET"]).expect_err("one element should fail");
        assert!(matches!(err, CommandError::TooFewArguments(1)));
    }

    #[test]
    fn non_utf8_command_name_errors() {
        let err = parse(&[b"\xff\xfe", b"mykey"]).expect_err("bad utf-8 name should fail");
        assert!(matches!(err, CommandError::InvalidUtf8(_)));
    }

    #[test]
    fn get_with_too_many_args_errors() {
        let err = parse(&[b"GET", b"mykey", b"extra"]).expect_err("GET takes one key");
        assert!(matches!(err, CommandError::TooManyArguments(3)));
    }

    #[test]
    fn set_with_too_few_args_errors() {
        let err = parse(&[b"SET", b"mykey"]).expect_err("SET needs a value");
        assert!(matches!(err, CommandError::TooFewArguments(2)));
    }

    #[test]
    fn set_with_too_many_args_errors() {
        let err =
            parse(&[b"SET", b"mykey", b"myval", b"extra"]).expect_err("SET takes key + value");
        assert!(matches!(err, CommandError::TooManyArguments(4)));
    }

    #[test]
    fn unrecognized_command_errors() {
        let err = parse(&[b"FOO", b"mykey"]).expect_err("FOO is not a command");
        assert!(matches!(err, CommandError::UnrecognizedCommand(name) if name == "FOO"));
    }

    fn assert_round_trips(command: Command) {
        let mut parser = crate::request::RequestParser::default();
        parser.push_bytes(&command.to_bytes());
        let request = parser
            .parse_next()
            .expect("serialized command should be valid RESP")
            .expect("serialized command should be a complete request");

        assert_eq!(Command::try_from(request).unwrap(), command);
    }

    #[test]
    fn round_trips_get() {
        assert_round_trips(Command::Get(b"mykey".to_vec()));
    }

    #[test]
    fn round_trips_set() {
        assert_round_trips(Command::Set(b"mykey".to_vec(), b"myval".to_vec()));
    }

    #[test]
    fn round_trips_del() {
        assert_round_trips(Command::Del(vec![b"k1".to_vec(), b"k2".to_vec()]));
    }
}
