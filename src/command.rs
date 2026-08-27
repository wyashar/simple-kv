use std::fmt;

use thiserror::Error;

use crate::command::CommandError::{
    TooFewArguments, TooManyArguments, UnevenArgumentLength, UnrecognizedCommand,
};
use crate::key_store::KeyStore;
use crate::request::Request;
use crate::response::Response;
use crate::util::Bytes;

const GET_STR: &str = "GET";
const SET_STR: &str = "SET";
const MGET_STR: &str = "MGET";
const MSET_STR: &str = "MSET";
const DEL_STR: &str = "DEL";
const GETALL_STR: &str = "GETALL";
const COMMAND_NAMES: [&str; 6] = [GET_STR, SET_STR, MGET_STR, MSET_STR, DEL_STR, GETALL_STR];

#[derive(Debug, PartialEq)]
pub enum Command {
    Get(Bytes),
    Set(Bytes, Bytes),
    MGet(Vec<Bytes>),
    MSet(Vec<(Bytes, Bytes)>),
    Del(Vec<Bytes>),
    GetAll,
}

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("command name was not valid utf-8")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    #[error("unrecognized command name: {0}, expected one of: {names:?}", names = COMMAND_NAMES)]
    UnrecognizedCommand(String),
    #[error("command has too few arguments; request had {0} elements")]
    TooFewArguments(usize),
    #[error("request had too many elements: {0}")]
    TooManyArguments(usize),
    #[error("expected even length, got len: {0}")]
    UnevenArgumentLength(usize),
}

impl Command {
    pub fn name(&self) -> &str {
        match self {
            Self::Del(_) => DEL_STR,
            Self::Set(_, _) => SET_STR,
            Self::Get(_) => GET_STR,
            Self::MGet(_) => MGET_STR,
            Self::MSet(_) => MSET_STR,
            Self::GetAll => GETALL_STR,
        }
    }

    pub fn is_set(&self) -> bool {
        self.name() == SET_STR
    }

    pub fn is_del(&self) -> bool {
        self.name() == DEL_STR
    }

    pub fn is_get(&self) -> bool {
        self.name() == GET_STR
    }

    pub fn is_mset(&self) -> bool {
        self.name() == MSET_STR
    }

    pub fn is_mget(&self) -> bool {
        self.name() == MGET_STR
    }

    pub fn is_write_op(&self) -> bool {
        self.is_del() || self.is_set() || self.is_mset()
    }

    pub fn apply(self, key_store: &mut KeyStore<Bytes, Bytes>) -> Response<Bytes> {
        match self {
            Self::Get(key) => key_store
                .get(&key)
                .map(|value| Response::Cstr(value.clone()))
                .unwrap_or(Response::Null),
            Self::MGet(keys) => Response::Array(
                keys.iter()
                    .map(|key| {
                        key_store
                            .get(key)
                            .map(|value| Response::Cstr(value.clone()))
                            .unwrap_or(Response::Null)
                    })
                    .collect(),
            ),
            Self::GetAll => Response::Array(
                key_store
                    .iter()
                    .map(|(key, value)| {
                        Response::Array(vec![
                            Response::Cstr(key.clone()),
                            Response::Cstr(value.clone()),
                        ])
                    })
                    .collect(),
            ),
            Self::Del(keys) => {
                let count = keys.iter().filter_map(|k| key_store.del(k)).count();
                Response::Integer(count as i64)
            }
            Self::Set(key, value) => {
                let _ = key_store.insert(key, value);
                Response::Ok
            }
            Self::MSet(entries) => {
                for (key, value) in entries {
                    let _ = key_store.insert(key, value);
                }
                Response::Ok
            }
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
            Self::MGet(keys) => parts.extend(keys.iter().map(Bytes::as_slice)),
            Self::MSet(entries) => {
                for (key, value) in entries {
                    parts.push(key);
                    parts.push(value);
                }
            }
            Self::Del(keys) => parts.extend(keys.iter().map(Bytes::as_slice)),
            Self::GetAll => {}
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
            Self::MGet(keys) => {
                for key in keys {
                    write!(f, " {}", String::from_utf8_lossy(key))?;
                }
            }
            Self::MSet(entries) => {
                for (key, value) in entries {
                    write!(
                        f,
                        " {} {}",
                        String::from_utf8_lossy(key),
                        String::from_utf8_lossy(value)
                    )?;
                }
            }
            Self::Del(keys) => {
                for key in keys {
                    write!(f, " {}", String::from_utf8_lossy(key))?;
                }
            }
            Self::GetAll => {}
        }

        Ok(())
    }
}

impl TryFrom<Request> for Command {
    type Error = CommandError;

    fn try_from(request: Request) -> Result<Self, CommandError> {
        let commands = request.into_args();

        if commands.is_empty() {
            return Err(TooFewArguments(commands.len()));
        }

        let total = commands.len();
        let mut args = commands.into_iter();
        let name_bytes = args.next().expect("non-empty request checked above");
        let command_name = str::from_utf8(&name_bytes)?;

        match command_name {
            GET_STR => {
                if args.len() == 0 {
                    return Err(TooFewArguments(total));
                }
                if args.len() > 1 {
                    return Err(TooManyArguments(total));
                }
                Ok(Self::Get(args.next().expect("one argument checked above")))
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
            MGET_STR => {
                if args.len() == 0 {
                    return Err(TooFewArguments(total));
                }
                Ok(Self::MGet(args.collect()))
            }
            MSET_STR => {
                if args.len() < 2 {
                    return Err(TooFewArguments(total));
                }
                if !args.len().is_multiple_of(2) {
                    return Err(UnevenArgumentLength(args.len()));
                }

                let mut entries = Vec::with_capacity(args.len() / 2);
                while let Some(key) = args.next() {
                    let value = args.next().expect("even argument count checked above");
                    entries.push((key, value));
                }
                Ok(Self::MSet(entries))
            }
            DEL_STR => {
                if args.len() == 0 {
                    return Err(TooFewArguments(total));
                }
                Ok(Self::Del(args.collect()))
            }
            GETALL_STR => {
                if args.len() != 0 {
                    return Err(TooManyArguments(total));
                }
                Ok(Self::GetAll)
            }
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
    fn parses_mget() {
        assert_eq!(
            parse(&[b"MGET", b"k1", b"k2"]).unwrap(),
            Command::MGet(vec![b"k1".to_vec(), b"k2".to_vec()]),
        );
    }

    #[test]
    fn parses_getall_without_arguments() {
        assert_eq!(parse(&[b"GETALL"]).unwrap(), Command::GetAll);
    }

    #[test]
    fn parses_mset() {
        assert_eq!(
            parse(&[b"MSET", b"k1", b"v1", b"k2", b"v2"]).unwrap(),
            Command::MSet(vec![
                (b"k1".to_vec(), b"v1".to_vec()),
                (b"k2".to_vec(), b"v2".to_vec()),
            ]),
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
    fn getall_with_arguments_errors() {
        let err = parse(&[b"GETALL", b"extra"]).expect_err("GETALL takes no arguments");
        assert!(matches!(err, CommandError::TooManyArguments(2)));
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
    fn mset_requires_complete_key_value_pairs() {
        let err = parse(&[b"MSET", b"k1", b"v1", b"k2"])
            .expect_err("MSET requires a value for every key");
        assert!(matches!(err, CommandError::UnevenArgumentLength(3)));
        assert_eq!(err.to_string(), "expected even length, got len: 3");
    }

    #[test]
    fn unrecognized_command_errors() {
        let err = parse(&[b"FOO", b"mykey"]).expect_err("FOO is not a command");
        assert!(matches!(err, CommandError::UnrecognizedCommand(name) if name == "FOO"));
    }

    fn assert_round_trips(command: Command) {
        let mut requests =
            crate::request::RequestReader::new(std::io::Cursor::new(command.to_bytes()));
        let request = requests
            .read_next()
            .expect("serialized command should be valid RESP")
            .expect("serialized command should be a complete request");

        assert_eq!(Command::try_from(request).unwrap(), command);
    }

    #[test]
    fn round_trips_get() {
        assert_round_trips(Command::Get(b"mykey".to_vec()));
    }

    #[test]
    fn round_trips_getall() {
        assert_round_trips(Command::GetAll);
    }

    #[test]
    fn round_trips_set() {
        assert_round_trips(Command::Set(b"mykey".to_vec(), b"myval".to_vec()));
    }

    #[test]
    fn round_trips_multi_key_commands() {
        assert_round_trips(Command::MGet(vec![b"k1".to_vec(), b"k2".to_vec()]));
        assert_round_trips(Command::MSet(vec![
            (b"k1".to_vec(), b"v1".to_vec()),
            (b"k2".to_vec(), b"v2".to_vec()),
        ]));
    }

    #[test]
    fn round_trips_del() {
        assert_round_trips(Command::Del(vec![b"k1".to_vec(), b"k2".to_vec()]));
    }

    #[test]
    fn classifies_read_and_write_commands() {
        assert!(Command::Get(b"k".to_vec()).is_get());
        assert!(!Command::Get(b"k".to_vec()).is_set());
        assert!(!Command::Get(b"k".to_vec()).is_del());
        assert!(!Command::Get(b"k".to_vec()).is_write_op());

        assert!(!Command::Set(b"k".to_vec(), b"v".to_vec()).is_get());
        assert!(Command::Set(b"k".to_vec(), b"v".to_vec()).is_write_op());

        assert!(!Command::Del(vec![b"k".to_vec()]).is_get());
        assert!(Command::Del(vec![b"k".to_vec()]).is_write_op());

        let mget = Command::MGet(vec![b"k".to_vec()]);
        assert!(mget.is_mget());
        assert!(!mget.is_mset());
        assert!(!mget.is_write_op());

        let mset = Command::MSet(vec![(b"k".to_vec(), b"v".to_vec())]);
        assert!(mset.is_mset());
        assert!(!mset.is_mget());
        assert!(mset.is_write_op());
    }

    #[test]
    fn applies_multi_key_commands() {
        let mut key_store = KeyStore::default();
        assert_eq!(
            Command::MSet(vec![
                (b"k1".to_vec(), b"v1".to_vec()),
                (b"k2".to_vec(), b"v2".to_vec()),
            ])
            .apply(&mut key_store),
            Response::Ok
        );
        assert_eq!(
            Command::MGet(vec![b"k2".to_vec(), b"missing".to_vec(), b"k1".to_vec()])
                .apply(&mut key_store),
            Response::Array(vec![
                Response::Cstr(b"v2".to_vec()),
                Response::Null,
                Response::Cstr(b"v1".to_vec()),
            ])
        );
    }

    #[test]
    fn getall_returns_nested_key_value_pairs() {
        let mut key_store = KeyStore::default();
        key_store.insert(b"k1".to_vec(), b"v1".to_vec());
        key_store.insert(b"k2".to_vec(), b"v2".to_vec());

        let Response::Array(pairs) = Command::GetAll.apply(&mut key_store) else {
            panic!("GETALL should return an array");
        };

        assert_eq!(pairs.len(), 2);
        assert!(pairs.contains(&Response::Array(vec![
            Response::Cstr(b"k1".to_vec()),
            Response::Cstr(b"v1".to_vec()),
        ])));
        assert!(pairs.contains(&Response::Array(vec![
            Response::Cstr(b"k2".to_vec()),
            Response::Cstr(b"v2".to_vec()),
        ])));
    }
}
