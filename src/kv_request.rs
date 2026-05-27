use std::{fmt, io::BufRead};
use strum::VariantNames;

pub struct KvRequest {
    command: KvCommand,
    // TODO: add some sort of top-levle metadata here, or collapse into just KvCommand
    // fields could be like request id, trace context, etc etc
}

enum KvParseError {
    InvalidKvCommandFormat(KvCommandParseError),
}

#[derive(Debug, PartialEq)]
pub enum KvCommandParseError {
    InvalidOperationEncoding,
    InvalidKeyLenEncoding,
    InvalidKeyEncoding,
    InvalidValueLenEncoding,
    InvalidValueEncoding,
    UnknownOperation(String),
}

#[derive(strum::VariantNames)]
#[strum(serialize_all = "PascalCase")]
pub enum KvCommand {
    Put(Vec<u8>, Vec<u8>),
    Get(Vec<u8>),
    Del(Vec<u8>),
}

impl KvRequest {
    pub fn from_reader<T: BufRead>(reader: &mut T) -> Result<KvRequest, KvParseError> {
        Ok(Self {
            command: KvCommand::from_reader(reader)
                .map_err(|e| KvParseError::InvalidKvCommandFormat(e))?,
        })
    }
}

impl KvCommand {
    pub fn from_reader<T: BufRead>(reader: &mut T) -> Result<KvCommand, KvCommandParseError> {
        let mut command_bytes: Vec<u8> = Vec::new();
        reader
            .read_until(b'\n', &mut command_bytes)
            .map_err(|_| KvCommandParseError::InvalidOperationEncoding)?;

        let command_bytes_trimmed = command_bytes
            .strip_suffix(b"\r\n")
            .ok_or(KvCommandParseError::InvalidOperationEncoding)?;
        let command_str: &str = std::str::from_utf8(command_bytes_trimmed)
            .map_err(|_| KvCommandParseError::InvalidOperationEncoding)
            .and_then(|s| match s {
                "Put" | "Get" | "Del" => Ok(s),
                _ => Err(KvCommandParseError::UnknownOperation(s.to_owned())),
            })?;

        let mut key_length_bytes: Vec<u8> = Vec::new();
        reader
            .read_until(b'\n', &mut key_length_bytes)
            .map_err(|_| KvCommandParseError::InvalidKeyLenEncoding)?;

        let key_length_bytes_trimmed = key_length_bytes
            .strip_suffix(b"\r\n")
            .ok_or(KvCommandParseError::InvalidKeyLenEncoding)?;
        let key_length: usize = std::str::from_utf8(key_length_bytes_trimmed)
            .map_err(|_| KvCommandParseError::InvalidKeyLenEncoding)?
            .parse()
            .map_err(|_| KvCommandParseError::InvalidKeyLenEncoding)?;

        let mut key_bytes: Vec<u8> = vec![0u8; key_length];

        reader
            .read_exact(&mut key_bytes)
            .map_err(|_| KvCommandParseError::InvalidKeyEncoding)?;

        let mut carriage_return: [u8; 2] = [0; 2];
        reader
            .read_exact(&mut carriage_return)
            .map_err(|_| KvCommandParseError::InvalidKeyEncoding)?;
        if &carriage_return != b"\r\n" {
            return Err(KvCommandParseError::InvalidKeyEncoding);
        }

        match command_str {
            "Get" => Ok(KvCommand::Get(key_bytes)),
            "Del" => Ok(KvCommand::Del(key_bytes)),
            "Put" => {
                let mut value_length_bytes: Vec<u8> = Vec::new();
                reader
                    .read_until(b'\n', &mut value_length_bytes)
                    .map_err(|_| KvCommandParseError::InvalidValueLenEncoding)?;

                let value_length_bytes_trimmed = value_length_bytes
                    .strip_suffix(b"\r\n")
                    .ok_or(KvCommandParseError::InvalidValueLenEncoding)?;
                let value_length: usize = std::str::from_utf8(value_length_bytes_trimmed)
                    .map_err(|_| KvCommandParseError::InvalidValueLenEncoding)?
                    .parse()
                    .map_err(|_| KvCommandParseError::InvalidValueLenEncoding)?;

                let mut value_bytes: Vec<u8> = vec![0u8; value_length];
                reader
                    .read_exact(&mut value_bytes)
                    .map_err(|_| KvCommandParseError::InvalidValueEncoding)?;

                let mut value_carriage_return: [u8; 2] = [0; 2];
                reader
                    .read_exact(&mut value_carriage_return)
                    .map_err(|_| KvCommandParseError::InvalidValueEncoding)?;
                if &value_carriage_return != b"\r\n" {
                    return Err(KvCommandParseError::InvalidValueEncoding);
                }

                Ok(KvCommand::Put(key_bytes, value_bytes))
            }
            _ => unreachable!("all variants of Operation were matched as strs during byte parsing"),
        }
    }
}

impl fmt::Display for KvCommandParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOperationEncoding => write!(
                f,
                "Failed to deserialize KvCommand, expected a command (e.g. Put, Get, etc) followed by a CRLF!"
            ),
            Self::InvalidKeyLenEncoding => write!(
                f,
                "Failed to deserialize KvCommand, expected usize for key length followed by a CRLF!"
            ),
            Self::InvalidKeyEncoding => write!(
                f,
                "Failed to deserialize KvCommand, expected bytes of <key_length> length followed by a CRLF!"
            ),
            Self::InvalidValueEncoding => write!(
                f,
                "Failed to deserialize KvCommand, expected bytes of <value_length> length followed by a CRLF!"
            ),
            Self::InvalidValueLenEncoding => write!(
                f,
                "Failed to deserialize KvCommand, expected usize for value length followed by a CRLF!"
            ),
            Self::UnknownOperation(unknown_operation) => write!(
                f,
                "Failed to deserialize KvCommand, expected one of {:?}, got: {unknown_operation}",
                KvCommand::VARIANTS
            ),
        }
    }
}
