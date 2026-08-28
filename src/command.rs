use std::fmt;

use thiserror::Error;

use crate::command::CommandError::{
    TooFewArguments, TooManyArguments, UnevenArgumentLength, UnrecognizedCommand,
};
use crate::key_store::KeyStore;
use crate::request::Request;
use crate::response::Response;
use crate::util::{Bytes, get_unix_timestamp};

const GET_NAME: &str = "GET";
const SET_NAME: &str = "SET";
const MGET_NAME: &str = "MGET";
const MSET_NAME: &str = "MSET";
const DEL_NAME: &str = "DEL";
const GETALL_NAME: &str = "GETALL";
// EXPIRE only exists externally, internally, EXPIRE is really just EXPIRE_AT (unix_now() + EXPIRE.ms)
const EXPIRE_NAME: &str = "EXPIRE";
const EXPIREAT_NAME: &str = "EXPIREAT";
const COMMAND_NAMES: [&str; 8] = [
    GET_NAME,
    SET_NAME,
    MGET_NAME,
    MSET_NAME,
    DEL_NAME,
    GETALL_NAME,
    EXPIRE_NAME,
    EXPIREAT_NAME,
];

type CommandArgs = std::vec::IntoIter<Bytes>;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StoredValue {
    pub bytes: Bytes,
    pub expires_at: Option<u64>,
}

impl StoredValue {
    pub fn new(bytes: Bytes) -> Self {
        Self {
            bytes,
            expires_at: None,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| expires_at <= get_unix_timestamp())
    }
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Get(Bytes),
    Set(Bytes, Bytes),
    MGet(Vec<Bytes>),
    MSet(Vec<(Bytes, Bytes)>),
    Del(Vec<Bytes>),
    GetAll,
    ExpireAt(Bytes, u64),
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
    #[error("expiration must be a valid integer, got: {0}")]
    InvalidExpiration(String),
}

impl Command {
    fn eject_if_expired(key_store: &mut KeyStore<Bytes, StoredValue>, key: &Bytes) {
        if key_store.get(key).is_some_and(StoredValue::is_expired) {
            let _ = key_store.del(key);
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Del(_) => DEL_NAME,
            Self::Set(_, _) => SET_NAME,
            Self::Get(_) => GET_NAME,
            Self::MGet(_) => MGET_NAME,
            Self::MSet(_) => MSET_NAME,
            Self::GetAll => GETALL_NAME,
            Self::ExpireAt(_, _) => EXPIREAT_NAME,
        }
    }

    pub fn is_set(&self) -> bool {
        self.name() == SET_NAME
    }

    pub fn is_del(&self) -> bool {
        self.name() == DEL_NAME
    }

    pub fn is_get(&self) -> bool {
        self.name() == GET_NAME
    }

    pub fn is_mset(&self) -> bool {
        self.name() == MSET_NAME
    }

    pub fn is_mget(&self) -> bool {
        self.name() == MGET_NAME
    }

    pub fn is_expire_at(&self) -> bool {
        self.name() == EXPIREAT_NAME
    }

    pub fn is_write_op(&self) -> bool {
        self.is_del() || self.is_set() || self.is_mset() || self.is_expire_at()
    }

    fn parse_get(mut args: CommandArgs, total: usize) -> Result<Self, CommandError> {
        if args.len() == 0 {
            return Err(TooFewArguments(total));
        }
        if args.len() > 1 {
            return Err(TooManyArguments(total));
        }

        Ok(Self::Get(args.next().expect("one argument checked above")))
    }

    fn parse_set(mut args: CommandArgs, total: usize) -> Result<Self, CommandError> {
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

    fn parse_mget(args: CommandArgs, total: usize) -> Result<Self, CommandError> {
        if args.len() == 0 {
            return Err(TooFewArguments(total));
        }

        Ok(Self::MGet(args.collect()))
    }

    fn parse_mset(mut args: CommandArgs, total: usize) -> Result<Self, CommandError> {
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

    fn parse_del(args: CommandArgs, total: usize) -> Result<Self, CommandError> {
        if args.len() == 0 {
            return Err(TooFewArguments(total));
        }

        Ok(Self::Del(args.collect()))
    }

    fn parse_getall(args: CommandArgs, total: usize) -> Result<Self, CommandError> {
        if args.len() != 0 {
            return Err(TooManyArguments(total));
        }

        Ok(Self::GetAll)
    }

    fn parse_expiration(mut args: CommandArgs, total: usize) -> Result<(Bytes, u64), CommandError> {
        if args.len() < 2 {
            return Err(TooFewArguments(total));
        }
        if args.len() > 2 {
            return Err(TooManyArguments(total));
        }

        let key = args.next().expect("len == 2 checked above");
        let expiration = args.next().expect("len == 2 checked above");
        let expiration_str = str::from_utf8(&expiration)?;
        let seconds = expiration_str
            .parse()
            .map_err(|_| CommandError::InvalidExpiration(expiration_str.to_owned()))?;

        Ok((key, seconds))
    }

    fn parse_expire(args: CommandArgs, total: usize) -> Result<Self, CommandError> {
        let (key, seconds) = Self::parse_expiration(args, total)?;
        Ok(Self::ExpireAt(
            key,
            get_unix_timestamp().saturating_add(seconds),
        ))
    }

    fn parse_expire_at(args: CommandArgs, total: usize) -> Result<Self, CommandError> {
        let (key, timestamp) = Self::parse_expiration(args, total)?;
        Ok(Self::ExpireAt(key, timestamp))
    }

    pub(crate) fn apply(self, key_store: &mut KeyStore<Bytes, StoredValue>) -> Response<Bytes> {
        match self {
            Self::Get(key) => Self::apply_get(key, key_store),
            Self::MGet(keys) => Self::apply_mget(keys, key_store),
            Self::GetAll => Self::apply_getall(key_store),
            Self::Del(keys) => Self::apply_del(keys, key_store),
            Self::Set(key, value) => Self::apply_set(key, value, key_store),
            Self::MSet(entries) => Self::apply_mset(entries, key_store),
            Self::ExpireAt(key, timestamp) => Self::apply_expire_at(key, timestamp, key_store),
        }
    }

    fn apply_get(key: Bytes, key_store: &mut KeyStore<Bytes, StoredValue>) -> Response<Bytes> {
        Self::eject_if_expired(key_store, &key);
        key_store
            .get(&key)
            .map(|value| Response::Cstr(value.bytes.clone()))
            .unwrap_or(Response::Null)
    }

    fn apply_mget(
        keys: Vec<Bytes>,
        key_store: &mut KeyStore<Bytes, StoredValue>,
    ) -> Response<Bytes> {
        Response::Array(
            keys.into_iter()
                .map(|key| Self::apply_get(key, key_store))
                .collect(),
        )
    }

    fn apply_getall(key_store: &mut KeyStore<Bytes, StoredValue>) -> Response<Bytes> {
        let expired_keys: Vec<_> = key_store
            .iter()
            .filter(|(_, value)| value.is_expired())
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired_keys {
            let _ = key_store.del(&key);
        }

        Response::Array(
            key_store
                .iter()
                .map(|(key, value)| {
                    Response::Array(vec![
                        Response::Cstr(key.clone()),
                        Response::Cstr(value.bytes.clone()),
                    ])
                })
                .collect(),
        )
    }

    fn apply_del(
        keys: Vec<Bytes>,
        key_store: &mut KeyStore<Bytes, StoredValue>,
    ) -> Response<Bytes> {
        let count = keys
            .iter()
            .filter(|key| {
                Self::eject_if_expired(key_store, key);
                key_store.del(key).is_some()
            })
            .count();
        Response::Integer(count as i64)
    }

    fn apply_set(
        key: Bytes,
        value: Bytes,
        key_store: &mut KeyStore<Bytes, StoredValue>,
    ) -> Response<Bytes> {
        let _ = key_store.insert(key, StoredValue::new(value));
        Response::Ok
    }

    fn apply_mset(
        entries: Vec<(Bytes, Bytes)>,
        key_store: &mut KeyStore<Bytes, StoredValue>,
    ) -> Response<Bytes> {
        for (key, value) in entries {
            let _ = key_store.insert(key, StoredValue::new(value));
        }
        Response::Ok
    }

    fn apply_expire_at(
        key: Bytes,
        timestamp: u64,
        key_store: &mut KeyStore<Bytes, StoredValue>,
    ) -> Response<Bytes> {
        Self::eject_if_expired(key_store, &key);

        let Some(value) = key_store.get_mut(&key) else {
            return Response::Integer(0);
        };

        value.expires_at = Some(timestamp);
        Response::Integer(1)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut parts: Vec<&[u8]> = vec![self.name().as_bytes()];
        let expire_ts;
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
            Self::ExpireAt(key, expire_timestamp) => {
                parts.push(key);
                expire_ts = expire_timestamp.to_string();
                parts.push(expire_ts.as_bytes());
            }
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
            Self::ExpireAt(key, timestamp) => {
                write!(f, " {} {timestamp}", String::from_utf8_lossy(key))?;
            }
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
            GET_NAME => Self::parse_get(args, total),
            SET_NAME => Self::parse_set(args, total),
            MGET_NAME => Self::parse_mget(args, total),
            MSET_NAME => Self::parse_mset(args, total),
            DEL_NAME => Self::parse_del(args, total),
            GETALL_NAME => Self::parse_getall(args, total),
            EXPIRE_NAME => Self::parse_expire(args, total),
            EXPIREAT_NAME => Self::parse_expire_at(args, total),
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
    fn parses_expire() {
        let before = get_unix_timestamp();
        let Command::ExpireAt(key, timestamp) =
            parse(&[b"EXPIRE", b"mykey", b"60"]).expect("EXPIRE should parse")
        else {
            panic!("EXPIRE should become EXPIREAT");
        };
        assert_eq!(key, b"mykey");
        assert!(timestamp >= before + 60);
        assert!(timestamp <= get_unix_timestamp() + 60);
    }

    #[test]
    fn parses_expire_at() {
        assert_eq!(
            parse(&[b"EXPIREAT", b"mykey", b"2000000000"]).unwrap(),
            Command::ExpireAt(b"mykey".to_vec(), 2_000_000_000),
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
    fn expire_validates_arguments() {
        let err = parse(&[b"EXPIRE", b"mykey"]).expect_err("EXPIRE needs a duration");
        assert!(matches!(err, CommandError::TooFewArguments(2)));

        let err = parse(&[b"EXPIRE", b"mykey", b"60", b"extra"])
            .expect_err("EXPIRE takes key + duration");
        assert!(matches!(err, CommandError::TooManyArguments(4)));

        let err = parse(&[b"EXPIRE", b"mykey", b"soon"]).expect_err("duration must be an integer");
        assert!(matches!(
            err,
            CommandError::InvalidExpiration(value) if value == "soon"
        ));

        let err = parse(&[b"EXPIRE", b"mykey", b"-1"]).expect_err("duration must not be negative");
        assert!(matches!(
            err,
            CommandError::InvalidExpiration(value) if value == "-1"
        ));
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
    fn round_trips_expire() {
        assert_round_trips(Command::ExpireAt(b"mykey".to_vec(), 2_000_000_000));
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

        let expire_at = Command::ExpireAt(b"k".to_vec(), 2_000_000_000);
        assert!(expire_at.is_expire_at());
        assert!(expire_at.is_write_op());
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
    fn apply_ejects_expired_entries() {
        let mut key_store = KeyStore::default();
        key_store.insert(
            b"expired".to_vec(),
            StoredValue {
                bytes: b"old".to_vec(),
                expires_at: Some(0),
            },
        );
        key_store.insert(
            b"untouched".to_vec(),
            StoredValue {
                bytes: b"old".to_vec(),
                expires_at: Some(0),
            },
        );
        key_store.insert(b"live".to_vec(), StoredValue::new(b"value".to_vec()));

        assert_eq!(
            Command::Get(b"expired".to_vec()).apply(&mut key_store),
            Response::Null
        );
        assert!(key_store.get(&b"expired".to_vec()).is_none());
        assert!(key_store.get(&b"untouched".to_vec()).is_some());
        assert_eq!(
            key_store.get(&b"live".to_vec()),
            Some(&StoredValue::new(b"value".to_vec()))
        );
    }

    #[test]
    fn expire_sets_ttl_for_existing_key() {
        let mut key_store = KeyStore::default();
        key_store.insert(b"key".to_vec(), StoredValue::new(b"value".to_vec()));
        let before = get_unix_timestamp();
        let command = parse(&[b"EXPIRE", b"key", b"60"]).expect("EXPIRE should parse");

        assert_eq!(command.apply(&mut key_store), Response::Integer(1));

        let expires_at = key_store
            .get(&b"key".to_vec())
            .and_then(|value| value.expires_at)
            .expect("expiration should be set");
        assert!(expires_at >= before + 60);
        assert!(expires_at <= get_unix_timestamp() + 60);
    }

    #[test]
    fn expire_at_sets_absolute_expiration() {
        let mut key_store = KeyStore::default();
        key_store.insert(b"key".to_vec(), StoredValue::new(b"value".to_vec()));

        assert_eq!(
            Command::ExpireAt(b"key".to_vec(), 2_000_000_000).apply(&mut key_store),
            Response::Integer(1)
        );
        assert_eq!(
            key_store
                .get(&b"key".to_vec())
                .and_then(|value| value.expires_at),
            Some(2_000_000_000)
        );
    }

    #[test]
    fn expire_returns_zero_for_missing_key() {
        let mut key_store = KeyStore::default();

        assert_eq!(
            Command::ExpireAt(b"missing".to_vec(), get_unix_timestamp() + 60).apply(&mut key_store),
            Response::Integer(0)
        );
    }

    #[test]
    fn expire_with_zero_seconds_makes_key_unavailable() {
        let mut key_store = KeyStore::default();
        key_store.insert(b"key".to_vec(), StoredValue::new(b"value".to_vec()));

        assert_eq!(
            Command::ExpireAt(b"key".to_vec(), get_unix_timestamp()).apply(&mut key_store),
            Response::Integer(1)
        );
        assert_eq!(
            Command::Get(b"key".to_vec()).apply(&mut key_store),
            Response::Null
        );
    }

    #[test]
    fn getall_returns_nested_key_value_pairs() {
        let mut key_store = KeyStore::default();
        key_store.insert(b"k1".to_vec(), StoredValue::new(b"v1".to_vec()));
        key_store.insert(b"k2".to_vec(), StoredValue::new(b"v2".to_vec()));

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
