use crate::kv_store::{KvStore, KvStoreResult};
use std::{fmt, io::BufRead};
use strum::VariantNames;

pub enum OperationView<'a> {
    Put { key: &'a [u8], value: &'a [u8] },
    Get { key: &'a [u8] },
    Del { key: &'a [u8] },
}

impl OperationView<'_> {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Put { .. } => "Put",
            Self::Get { .. } => "Get",
            Self::Del { .. } => "Del",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    kind: OperationKind,
}

#[derive(Debug, Clone, PartialEq, strum::VariantNames)]
#[strum(serialize_all = "PascalCase")]
enum OperationKind {
    Put(Vec<u8>, Vec<u8>),
    Get(Vec<u8>),
    Del(Vec<u8>),
}

impl Operation {
    pub fn from_reader<Reader: BufRead>(
        reader: &mut Reader,
    ) -> Result<Operation, OperationParseError> {
        let mut operation_bytes: Vec<u8> = Vec::new();
        reader
            .read_until(b'\n', &mut operation_bytes)
            .map_err(|_| OperationParseError::InvalidOperationEncoding)?;

        let operation_bytes_trimmed = operation_bytes
            .strip_suffix(b"\r\n")
            .ok_or(OperationParseError::InvalidOperationEncoding)?;
        let operation_str: &str = std::str::from_utf8(operation_bytes_trimmed)
            .map_err(|_| OperationParseError::InvalidOperationEncoding)
            .and_then(|s| match s {
                "Put" | "Get" | "Del" => Ok(s),
                _ => Err(OperationParseError::UnknownOperation(s.to_owned())),
            })?;

        let mut key_length_bytes: Vec<u8> = Vec::new();
        reader
            .read_until(b'\n', &mut key_length_bytes)
            .map_err(|_| OperationParseError::InvalidKeyLenEncoding)?;

        let key_length_bytes_trimmed = key_length_bytes
            .strip_suffix(b"\r\n")
            .ok_or(OperationParseError::InvalidKeyLenEncoding)?;
        let key_length: usize = std::str::from_utf8(key_length_bytes_trimmed)
            .map_err(|_| OperationParseError::InvalidKeyLenEncoding)?
            .parse()
            .map_err(|_| OperationParseError::InvalidKeyLenEncoding)?;

        let mut key_bytes: Vec<u8> = vec![0u8; key_length];

        reader
            .read_exact(&mut key_bytes)
            .map_err(|_| OperationParseError::InvalidKeyEncoding)?;

        let mut carriage_return: [u8; 2] = [0; 2];
        reader
            .read_exact(&mut carriage_return)
            .map_err(|_| OperationParseError::InvalidKeyEncoding)?;
        if &carriage_return != b"\r\n" {
            return Err(OperationParseError::InvalidKeyEncoding);
        }

        match operation_str {
            "Get" => Ok(Operation::get(key_bytes)),
            "Del" => Ok(Operation::del(key_bytes)),
            "Put" => {
                let mut value_length_bytes: Vec<u8> = Vec::new();
                reader
                    .read_until(b'\n', &mut value_length_bytes)
                    .map_err(|_| OperationParseError::InvalidValueLenEncoding)?;

                let value_length_bytes_trimmed = value_length_bytes
                    .strip_suffix(b"\r\n")
                    .ok_or(OperationParseError::InvalidValueLenEncoding)?;
                let value_length: usize = std::str::from_utf8(value_length_bytes_trimmed)
                    .map_err(|_| OperationParseError::InvalidValueLenEncoding)?
                    .parse()
                    .map_err(|_| OperationParseError::InvalidValueLenEncoding)?;

                let mut value_bytes: Vec<u8> = vec![0u8; value_length];
                reader
                    .read_exact(&mut value_bytes)
                    .map_err(|_| OperationParseError::InvalidValueEncoding)?;

                let mut value_carriage_return: [u8; 2] = [0; 2];
                reader
                    .read_exact(&mut value_carriage_return)
                    .map_err(|_| OperationParseError::InvalidValueEncoding)?;
                if &value_carriage_return != b"\r\n" {
                    return Err(OperationParseError::InvalidValueEncoding);
                }

                Ok(Operation::put(key_bytes, value_bytes))
            }
            _ => unreachable!("all variants of Operation were matched as strs during byte parsing"),
        }
    }

    fn put(key: Vec<u8>, value: Vec<u8>) -> Self {
        Operation {
            kind: OperationKind::Put(key, value),
        }
    }

    fn get(key: Vec<u8>) -> Self {
        Operation {
            kind: OperationKind::Get(key),
        }
    }

    fn del(key: Vec<u8>) -> Self {
        Operation {
            kind: OperationKind::Del(key),
        }
    }

    pub fn name(&self) -> &'static str {
        match &self.kind {
            OperationKind::Put(_, _) => "Put",
            OperationKind::Get(_) => "Get",
            OperationKind::Del(_) => "Del",
        }
    }

    pub fn as_view(&self) -> OperationView<'_> {
        match &self.kind {
            OperationKind::Put(key, value) => OperationView::Put { key, value },
            OperationKind::Get(key) => OperationView::Get { key },
            OperationKind::Del(key) => OperationView::Del { key },
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.into()
    }

    pub fn execute(self, store: &mut KvStore) -> KvStoreResult {
        match self.kind {
            OperationKind::Put(key, value) => store.put(key, value),
            OperationKind::Get(key) => store.get(&key),
            OperationKind::Del(key) => store.del(&key),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WireFormat {
    kind: WireFormatKind,
}

#[derive(Debug, Clone, PartialEq, strum::VariantNames)]
enum WireFormatKind {
    Cmd(Operation),
    SimpleString(String),
}

impl WireFormat {
    fn cmd(op: Operation) -> Self {
        WireFormat {
            kind: WireFormatKind::Cmd(op),
        }
    }

    fn simple_string(s: String) -> Self {
        WireFormat {
            kind: WireFormatKind::SimpleString(s),
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.into()
    }

    pub fn into_command(self) -> Option<Operation> {
        match self.kind {
            WireFormatKind::Cmd(op) => Some(op),
            WireFormatKind::SimpleString(_) => None,
        }
    }

    pub fn from_reader<Reader: BufRead>(reader: &mut Reader) -> Result<Self, WireFormatParseError> {
        let mut first_line_bytes: Vec<u8> = Vec::new();
        reader
            .read_until(b'\n', &mut first_line_bytes)
            .map_err(|_| WireFormatParseError::InvalidTypeEncoding)?;

        let first_line_trimmed = first_line_bytes
            .strip_suffix(b"\r\n")
            .ok_or(WireFormatParseError::InvalidTypeEncoding)?;
        let first_line_str: &str = std::str::from_utf8(first_line_trimmed)
            .map_err(|_| WireFormatParseError::InvalidTypeEncoding)
            .and_then(|s| match s {
                "op" | "sstr" => Ok(s),
                _ => Err(WireFormatParseError::UnknownType(s.to_owned())),
            })?;

        match first_line_str {
            "op" => {
                let operation = Operation::from_reader(reader)
                    .map_err(WireFormatParseError::InvalidCmdEncoding)?;
                Ok(WireFormat::cmd(operation))
            }
            "sstr" => {
                let mut simple_str_bytes: Vec<u8> = Vec::new();
                reader
                    .read_until(b'\n', &mut simple_str_bytes)
                    .map_err(|_| WireFormatParseError::InvalidSimpleStringEncoding)?;

                let simple_str_trimmed = simple_str_bytes
                    .strip_suffix(b"\r\n")
                    .ok_or(WireFormatParseError::InvalidSimpleStringEncoding)?;
                let simple_str = std::str::from_utf8(simple_str_trimmed)
                    .map_err(|_| WireFormatParseError::InvalidSimpleStringEncoding)?
                    .to_string();

                Ok(WireFormat::simple_string(simple_str))
            }
            _ => unreachable!("all WireFormat type identifiers were validated during type parsing"),
        }
    }
}

impl fmt::Display for Operation {
    // NOTE: This is purely for human readable string representations of the Operation
    // Operation => String => Operation may fail because the keys and values are not byte safe,
    // therefore not guarenteed to be Valid UTF8!
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            OperationKind::Put(key, value) => {
                let key_utf8_lossy = String::from_utf8_lossy(key);
                let value_utf8_losssy = String::from_utf8_lossy(value);
                write!(
                    f,
                    "Put\r\n{}\r\n{}\r\n{}\r\n{}\r\n",
                    key_utf8_lossy.len(),
                    key_utf8_lossy,
                    value_utf8_losssy.len(),
                    value_utf8_losssy
                )
            }
            OperationKind::Get(key) => {
                let key_utf8_lossy = String::from_utf8_lossy(key);
                write!(
                    f,
                    "Get\r\n{}\r\n{}\r\n",
                    key_utf8_lossy.len(),
                    key_utf8_lossy
                )
            }
            OperationKind::Del(key) => {
                let key_utf8_lossy = String::from_utf8_lossy(key);
                write!(
                    f,
                    "Del\r\n{}\r\n{}\r\n",
                    key_utf8_lossy.len(),
                    key_utf8_lossy
                )
            }
        }
    }
}

impl fmt::Display for WireFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            WireFormatKind::Cmd(op) => write!(f, "op\r\n{}", op),
            WireFormatKind::SimpleString(s) => write!(f, "sstr\r\n{}\r\n", s),
        }
    }
}

impl From<Operation> for Vec<u8> {
    fn from(op: Operation) -> Self {
        let mut buf = Vec::new();
        match op.kind {
            OperationKind::Put(key, value) => {
                buf.extend_from_slice(b"Put\r\n");
                buf.extend_from_slice(key.len().to_string().as_bytes());
                buf.extend_from_slice(b"\r\n");
                buf.extend_from_slice(&key);
                buf.extend_from_slice(b"\r\n");
                buf.extend_from_slice(value.len().to_string().as_bytes());
                buf.extend_from_slice(b"\r\n");
                buf.extend_from_slice(&value);
                buf.extend_from_slice(b"\r\n");
            }
            OperationKind::Get(key) => {
                buf.extend_from_slice(b"Get\r\n");
                buf.extend_from_slice(key.len().to_string().as_bytes());
                buf.extend_from_slice(b"\r\n");
                buf.extend_from_slice(&key);
                buf.extend_from_slice(b"\r\n");
            }
            OperationKind::Del(key) => {
                buf.extend_from_slice(b"Del\r\n");
                buf.extend_from_slice(key.len().to_string().as_bytes());
                buf.extend_from_slice(b"\r\n");
                buf.extend_from_slice(&key);
                buf.extend_from_slice(b"\r\n");
            }
        }
        buf
    }
}

impl From<WireFormat> for Vec<u8> {
    fn from(wf: WireFormat) -> Self {
        let mut buf = Vec::new();
        match wf.kind {
            WireFormatKind::Cmd(op) => {
                buf.extend_from_slice(b"op\r\n");
                buf.extend(Vec::<u8>::from(op));
            }
            WireFormatKind::SimpleString(s) => {
                buf.extend_from_slice(b"sstr\r\n");
                buf.extend_from_slice(s.as_bytes());
                buf.extend_from_slice(b"\r\n");
            }
        }
        buf
    }
}

#[derive(Debug, PartialEq)]
pub enum WireFormatParseError {
    InvalidTypeEncoding,
    UnknownType(String),
    InvalidCmdEncoding(OperationParseError),
    InvalidSimpleStringEncoding,
}

impl fmt::Display for WireFormatParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTypeEncoding => write!(
                f,
                "Failed to deserialize WireFormat, expected a type identifier (one of {:?}) followed by a CRLF!",
                WireFormatKind::VARIANTS
            ),
            Self::UnknownType(unknown_type) => write!(
                f,
                "Failed to deserialize WireFormat, expected one of {:?}, got: {unknown_type}",
                WireFormatKind::VARIANTS
            ),
            Self::InvalidCmdEncoding(err) => write!(f, "{err}"),
            Self::InvalidSimpleStringEncoding => write!(
                f,
                "Failed to deserialize WireFormat, expected a UTF-8 string followed by a CRLF!"
            ),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum OperationParseError {
    InvalidOperationEncoding,
    InvalidKeyLenEncoding,
    InvalidKeyEncoding,
    InvalidValueLenEncoding,
    InvalidValueEncoding,
    UnknownOperation(String),
}

impl fmt::Display for OperationParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOperationEncoding => write!(
                f,
                "Failed to deserialize Operation, expected a command (e.g. Put, Get, etc) followed by a CRLF!"
            ),
            Self::InvalidKeyLenEncoding => write!(
                f,
                "Failed to deserialize Operation, expected usize for key length followed by a CRLF!"
            ),
            Self::InvalidKeyEncoding => write!(
                f,
                "Failed to deserialize Operation, expected bytes of <key_length> length followed by a CRLF!"
            ),
            Self::InvalidValueEncoding => write!(
                f,
                "Failed to deserialize Operation, expected bytes of <value_length> length followed by a CRLF!"
            ),
            Self::InvalidValueLenEncoding => write!(
                f,
                "Failed to deserialize Operation, expected usize for value length followed by a CRLF!"
            ),
            Self::UnknownOperation(unknown_operation) => write!(
                f,
                "Failed to deserialize Operation, expected one of {:?}, got: {unknown_operation}",
                OperationKind::VARIANTS
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_reader_for_operation_bad_operation_bytes() {
        let mut bad_bytes: &[u8] = b"hello!\r\n".as_slice();
        let actual = Operation::from_reader(&mut bad_bytes);
        let expected: Result<Operation, OperationParseError> =
            Err(OperationParseError::UnknownOperation("hello!".to_owned()));

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_operation_empty_byte_arr() {
        let mut empty_byte_arr: &[u8] = b"".as_slice();
        let actual = Operation::from_reader(&mut empty_byte_arr);
        let expected: Result<Operation, OperationParseError> =
            Err(OperationParseError::InvalidOperationEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_operation_bad_operator() {
        let mut byte_arry: &[u8] = b"InvalidOperation\r\n";
        let actual = Operation::from_reader(&mut byte_arry);
        let expected: Result<Operation, OperationParseError> = Err(
            OperationParseError::UnknownOperation("InvalidOperation".to_owned()),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_operation_bad_key_len() {
        let mut byte_arr: &[u8] = b"Put\r\nHello\r\n";
        let actual = Operation::from_reader(&mut byte_arr);
        let expected: Result<Operation, OperationParseError> =
            Err(OperationParseError::InvalidKeyLenEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_operation_mismatch_key_len() {
        let mut byte_arr: &[u8] = b"Get\r\n5\r\nNotFive\r\n";
        let actual = Operation::from_reader(&mut byte_arr);
        let expected: Result<Operation, OperationParseError> =
            Err(OperationParseError::InvalidKeyEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_operation_bad_value_len_encoding() {
        let mut byte_arr: &[u8] = b"Put\r\n5\r\n12345\r\nInvalidLen";
        let actual = Operation::from_reader(&mut byte_arr);
        let expected: Result<Operation, OperationParseError> =
            Err(OperationParseError::InvalidValueLenEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_operation_value_len_mismatch() {
        let mut byte_arr: &[u8] = b"Put\r\n5\r\n12345\r\n6\r\nSeven";
        let actual = Operation::from_reader(&mut byte_arr);
        let expected: Result<Operation, OperationParseError> =
            Err(OperationParseError::InvalidValueEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_operation_put_valid() {
        let mut byte_arr: &[u8] = b"Put\r\n6\r\nKey123\r\n7\r\nValue12\r\n";
        let actual = Operation::from_reader(&mut byte_arr);

        let key_bytes: Vec<u8> = b"Key123".to_vec();
        let value_bytes: Vec<u8> = b"Value12".to_vec();
        let expected: Result<Operation, OperationParseError> =
            Ok(Operation::put(key_bytes, value_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_operation_get_valid() {
        let mut byte_arr: &[u8] = b"Get\r\n17\r\nDrakeIsABadArtist\r\n";
        let actual = Operation::from_reader(&mut byte_arr);

        let key_bytes: Vec<u8> = b"DrakeIsABadArtist".to_vec();
        let expected: Result<Operation, OperationParseError> = Ok(Operation::get(key_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_del_valid() {
        let mut byte_arr: &[u8] = b"Del\r\n4\r\nTree\r\n";
        let actual = Operation::from_reader(&mut byte_arr);

        let key_bytes: Vec<u8> = b"Tree".to_vec();
        let expected: Result<Operation, OperationParseError> = Ok(Operation::del(key_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_non_byte_safe_string() {
        // 𝕳𝖊𝖑𝖑𝖔 Wörld! ñoño 日本語 中文 한국어 العربية עברית ℃ ™ © ® € £ ¥ ✓ ← ↑ → ↓ ♠ ♣ ♥ ♦
        let mut byte_arr: &[u8] = b"Put\r\n5\r\nMyKey\r\n148\r\n\xf0\x9d\x95\xb3\xf0\x9d\x96\x8a\xf0\x9d\x96\x91\xf0\x9d\x96\x91\xf0\x9d\x96\x94 W\xc3\xb6rld! \xc3\xb1o\xc3\xb1o \xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e \xe4\xb8\xad\xe6\x96\x87 \xed\x95\x9c\xea\xb5\xad\xec\x96\xb4 \xd8\xa7\xd9\x84\xd8\xb9\xd8\xb1\xd8\xa8\xd9\x8a\xd8\xa9 \xd7\xa2\xd7\x91\xd7\xa8\xd7\x99\xd7\xaa \xe2\x84\x83 \xe2\x84\xa2 \xc2\xa9 \xc2\xae \xe2\x82\xac \xc2\xa3 \xc2\xa5 \xe2\x9c\x93 \xe2\x86\x90 \xe2\x86\x91 \xe2\x86\x92 \xe2\x86\x93 \xe2\x99\xa0 \xe2\x99\xa3 \xe2\x99\xa5 \xe2\x99\xa6\r\n";
        let actual = Operation::from_reader(&mut byte_arr);

        let key_bytes: Vec<u8> = b"MyKey".to_vec();
        let value_bytes: Vec<u8> = b"\xf0\x9d\x95\xb3\xf0\x9d\x96\x8a\xf0\x9d\x96\x91\xf0\x9d\x96\x91\xf0\x9d\x96\x94 W\xc3\xb6rld! \xc3\xb1o\xc3\xb1o \xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e \xe4\xb8\xad\xe6\x96\x87 \xed\x95\x9c\xea\xb5\xad\xec\x96\xb4 \xd8\xa7\xd9\x84\xd8\xb9\xd8\xb1\xd8\xa8\xd9\x8a\xd8\xa9 \xd7\xa2\xd7\x91\xd7\xa8\xd7\x99\xd7\xaa \xe2\x84\x83 \xe2\x84\xa2 \xc2\xa9 \xc2\xae \xe2\x82\xac \xc2\xa3 \xc2\xa5 \xe2\x9c\x93 \xe2\x86\x90 \xe2\x86\x91 \xe2\x86\x92 \xe2\x86\x93 \xe2\x99\xa0 \xe2\x99\xa3 \xe2\x99\xa5 \xe2\x99\xa6".to_vec();
        let expected: Result<Operation, OperationParseError> =
            Ok(Operation::put(key_bytes, value_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_operation_zalgo_key_del() {
        // Z̸̡̢̛̛̺̙͔̮͍̺̘̣̺̖͚̬̖͙͍̣̘̤̟̪̦̬͕̖̩̠͕͖̤̟̱̙̼̳͙̬̦̳͉̦̻̙̥̗̘͇͍̤̫̫͎̱̰͈̺̜̤͔̀͐͌̀͂͗̈́̌̅̊͑̋̒͒͊̀̓̏͊͌̎̈́̀̈́͘͘͜͠͝͠͝ͅͅą̷̧̧̢̛̛̛̹̟͎͉̝̩̬͚͖̝̩̱̩͕͔̖͇̘͇̗̯̙̣͙̮̙̗̹̺͕̱̰̱̲̬̞̤̳̹͍̝͕͑͒̒̐̓̐̃̃̽̏̾̆̋͌͌̒́̋̅́̏͒͗̎̒̔̑̀͑̿̄͑͑̿̈́͋̕̕͜͠͠ͅl̴͇̜̣̬̝̮̭̟͇͚̖͈͎͚͕͔͕͚̹̺̲̙̺̹͂͌̓̂̀̾̅̀̉̒̃̑̋̅͘̕͜͝g̷͉̲̙̥̜̟͔͓̰̯͇̮͎͓̈͛̒̊͐͛̃̊̆̂̎̈́͌͆̀͊̌̃̍̃̿͝͝ͅo̷̧̤̤̮̟̮̻̟̪̱̬͎̟̙̝͔͙̲̎̒̅̔̉͗̈́͛͊̈͆̀̈́͊͑̎̐̌̒͆͊̕̕̚
        let mut byte_arr: &[u8] = b"Del\r\n644\r\n\x5a\xcc\xb8\xcc\xa1\xcc\xa2\xcc\x9b\xcc\x9b\xcc\xba\xcc\x99\xcd\x94\xcc\xae\xcd\x8d\xcc\xba\xcc\x98\xcc\xa3\xcc\xba\xcc\x96\xcd\x9a\xcc\xac\xcc\x96\xcd\x99\xcd\x8d\xcc\xa3\xcc\x98\xcc\xa4\xcc\x9f\xcc\xaa\xcc\xa6\xcc\xac\xcd\x95\xcc\x96\xcc\xa9\xcc\xa0\xcd\x95\xcd\x96\xcc\xa4\xcc\x9f\xcc\xb1\xcc\x99\xcc\xbc\xcc\xb3\xcd\x99\xcc\xac\xcc\xa6\xcc\xb3\xcd\x89\xcc\xa6\xcc\xbb\xcc\x99\xcc\xa5\xcc\x97\xcc\x98\xcd\x87\xcd\x8d\xcc\xa4\xcc\xab\xcc\xab\xcd\x8e\xcc\xb1\xcc\xb0\xcd\x88\xcc\xba\xcc\x9c\xcc\xa4\xcd\x94\xcc\x80\xcd\x90\xcd\x8c\xcc\x80\xcd\x82\xcd\x97\xcc\x88\xcc\x81\xcc\x8c\xcc\x85\xcc\x8a\xcd\x91\xcc\x8b\xcc\x92\xcd\x92\xcd\x8a\xcc\x80\xcc\x93\xcc\x8f\xcd\x8a\xcd\x8c\xcc\x8e\xcc\x88\xcc\x81\xcc\x80\xcc\x88\xcc\x81\xcd\x98\xcd\x98\xcd\x9c\xcd\xa0\xcd\x9d\xcd\xa0\xcd\x9d\xcd\x85\xcd\x85\xc4\x85\xcc\xb7\xcc\xa7\xcc\xa7\xcc\xa2\xcc\x9b\xcc\x9b\xcc\x9b\xcc\xb9\xcc\x9f\xcd\x8e\xcd\x89\xcc\x9d\xcc\xa9\xcc\xac\xcd\x9a\xcd\x96\xcc\x9d\xcc\xa9\xcc\xb1\xcc\xa9\xcd\x95\xcd\x94\xcc\x96\xcd\x87\xcc\x98\xcd\x87\xcc\x97\xcc\xaf\xcc\x99\xcc\xa3\xcd\x99\xcc\xae\xcc\x99\xcc\x97\xcc\xb9\xcc\xba\xcd\x95\xcc\xb1\xcc\xb0\xcc\xb1\xcc\xb2\xcc\xac\xcc\x9e\xcc\xa4\xcc\xb3\xcc\xb9\xcd\x8d\xcc\x9d\xcd\x95\xcd\x91\xcd\x92\xcc\x92\xcc\x90\xcc\x93\xcc\x90\xcc\x83\xcc\x83\xcc\xbd\xcc\x8f\xcc\xbe\xcc\x86\xcc\x8b\xcd\x8c\xcd\x8c\xcc\x92\xcc\x81\xcc\x8b\xcc\x85\xcc\x81\xcc\x8f\xcd\x92\xcd\x97\xcc\x8e\xcc\x92\xcc\x94\xcc\x91\xcc\x80\xcd\x91\xcc\xbf\xcc\x84\xcd\x91\xcd\x91\xcc\xbf\xcc\x88\xcc\x81\xcd\x8b\xcc\x95\xcc\x95\xcd\x9c\xcd\xa0\xcd\xa0\xcd\x85\x6c\xcc\xb4\xcd\x87\xcc\x9c\xcc\xa3\xcc\xac\xcc\x9d\xcc\xae\xcc\xad\xcc\x9f\xcd\x87\xcd\x9a\xcc\x96\xcd\x88\xcd\x8e\xcd\x9a\xcd\x95\xcd\x94\xcd\x95\xcd\x9a\xcc\xb9\xcc\xba\xcc\xb2\xcc\x99\xcc\xba\xcc\xb9\xcd\x82\xcd\x8c\xcc\x93\xcc\x82\xcc\x80\xcc\xbe\xcc\x85\xcc\x80\xcc\x89\xcc\x92\xcc\x83\xcc\x91\xcc\x8b\xcc\x85\xcd\x98\xcc\x95\xcd\x9c\xcd\x9d\x67\xcc\xb7\xcd\x89\xcc\xb2\xcc\x99\xcc\xa5\xcc\x9c\xcc\x9f\xcd\x94\xcd\x93\xcc\xb0\xcc\xaf\xcd\x87\xcc\xae\xcd\x8e\xcd\x93\xcc\x88\xcd\x9b\xcc\x92\xcc\x8a\xcd\x90\xcd\x9b\xcc\x83\xcc\x8a\xcc\x86\xcc\x82\xcc\x8e\xcc\x88\xcc\x81\xcd\x8c\xcd\x86\xcc\x80\xcd\x8a\xcc\x8c\xcc\x83\xcc\x8d\xcc\x83\xcc\xbf\xcd\x9d\xcd\x9d\xcd\x85\x6f\xcc\xb7\xcc\xa7\xcc\xa4\xcc\xa4\xcc\xae\xcc\x9f\xcc\xae\xcc\xbb\xcc\x9f\xcc\xaa\xcc\xb1\xcc\xac\xcd\x8e\xcc\x9f\xcc\x99\xcc\x9d\xcd\x94\xcd\x99\xcc\xb2\xcc\x8e\xcc\x92\xcc\x85\xcc\x94\xcc\x89\xcd\x97\xcc\x88\xcc\x81\xcd\x9b\xcd\x8a\xcc\x88\xcd\x86\xcc\x80\xcc\x88\xcc\x81\xcd\x8a\xcd\x91\xcc\x8e\xcc\x90\xcc\x8c\xcc\x92\xcd\x86\xcd\x8a\xcc\x95\xcc\x95\xcc\x9a\r\n";
        let actual = Operation::from_reader(&mut byte_arr);

        let key_bytes: Vec<u8> = b"\x5a\xcc\xb8\xcc\xa1\xcc\xa2\xcc\x9b\xcc\x9b\xcc\xba\xcc\x99\xcd\x94\xcc\xae\xcd\x8d\xcc\xba\xcc\x98\xcc\xa3\xcc\xba\xcc\x96\xcd\x9a\xcc\xac\xcc\x96\xcd\x99\xcd\x8d\xcc\xa3\xcc\x98\xcc\xa4\xcc\x9f\xcc\xaa\xcc\xa6\xcc\xac\xcd\x95\xcc\x96\xcc\xa9\xcc\xa0\xcd\x95\xcd\x96\xcc\xa4\xcc\x9f\xcc\xb1\xcc\x99\xcc\xbc\xcc\xb3\xcd\x99\xcc\xac\xcc\xa6\xcc\xb3\xcd\x89\xcc\xa6\xcc\xbb\xcc\x99\xcc\xa5\xcc\x97\xcc\x98\xcd\x87\xcd\x8d\xcc\xa4\xcc\xab\xcc\xab\xcd\x8e\xcc\xb1\xcc\xb0\xcd\x88\xcc\xba\xcc\x9c\xcc\xa4\xcd\x94\xcc\x80\xcd\x90\xcd\x8c\xcc\x80\xcd\x82\xcd\x97\xcc\x88\xcc\x81\xcc\x8c\xcc\x85\xcc\x8a\xcd\x91\xcc\x8b\xcc\x92\xcd\x92\xcd\x8a\xcc\x80\xcc\x93\xcc\x8f\xcd\x8a\xcd\x8c\xcc\x8e\xcc\x88\xcc\x81\xcc\x80\xcc\x88\xcc\x81\xcd\x98\xcd\x98\xcd\x9c\xcd\xa0\xcd\x9d\xcd\xa0\xcd\x9d\xcd\x85\xcd\x85\xc4\x85\xcc\xb7\xcc\xa7\xcc\xa7\xcc\xa2\xcc\x9b\xcc\x9b\xcc\x9b\xcc\xb9\xcc\x9f\xcd\x8e\xcd\x89\xcc\x9d\xcc\xa9\xcc\xac\xcd\x9a\xcd\x96\xcc\x9d\xcc\xa9\xcc\xb1\xcc\xa9\xcd\x95\xcd\x94\xcc\x96\xcd\x87\xcc\x98\xcd\x87\xcc\x97\xcc\xaf\xcc\x99\xcc\xa3\xcd\x99\xcc\xae\xcc\x99\xcc\x97\xcc\xb9\xcc\xba\xcd\x95\xcc\xb1\xcc\xb0\xcc\xb1\xcc\xb2\xcc\xac\xcc\x9e\xcc\xa4\xcc\xb3\xcc\xb9\xcd\x8d\xcc\x9d\xcd\x95\xcd\x91\xcd\x92\xcc\x92\xcc\x90\xcc\x93\xcc\x90\xcc\x83\xcc\x83\xcc\xbd\xcc\x8f\xcc\xbe\xcc\x86\xcc\x8b\xcd\x8c\xcd\x8c\xcc\x92\xcc\x81\xcc\x8b\xcc\x85\xcc\x81\xcc\x8f\xcd\x92\xcd\x97\xcc\x8e\xcc\x92\xcc\x94\xcc\x91\xcc\x80\xcd\x91\xcc\xbf\xcc\x84\xcd\x91\xcd\x91\xcc\xbf\xcc\x88\xcc\x81\xcd\x8b\xcc\x95\xcc\x95\xcd\x9c\xcd\xa0\xcd\xa0\xcd\x85\x6c\xcc\xb4\xcd\x87\xcc\x9c\xcc\xa3\xcc\xac\xcc\x9d\xcc\xae\xcc\xad\xcc\x9f\xcd\x87\xcd\x9a\xcc\x96\xcd\x88\xcd\x8e\xcd\x9a\xcd\x95\xcd\x94\xcd\x95\xcd\x9a\xcc\xb9\xcc\xba\xcc\xb2\xcc\x99\xcc\xba\xcc\xb9\xcd\x82\xcd\x8c\xcc\x93\xcc\x82\xcc\x80\xcc\xbe\xcc\x85\xcc\x80\xcc\x89\xcc\x92\xcc\x83\xcc\x91\xcc\x8b\xcc\x85\xcd\x98\xcc\x95\xcd\x9c\xcd\x9d\x67\xcc\xb7\xcd\x89\xcc\xb2\xcc\x99\xcc\xa5\xcc\x9c\xcc\x9f\xcd\x94\xcd\x93\xcc\xb0\xcc\xaf\xcd\x87\xcc\xae\xcd\x8e\xcd\x93\xcc\x88\xcd\x9b\xcc\x92\xcc\x8a\xcd\x90\xcd\x9b\xcc\x83\xcc\x8a\xcc\x86\xcc\x82\xcc\x8e\xcc\x88\xcc\x81\xcd\x8c\xcd\x86\xcc\x80\xcd\x8a\xcc\x8c\xcc\x83\xcc\x8d\xcc\x83\xcc\xbf\xcd\x9d\xcd\x9d\xcd\x85\x6f\xcc\xb7\xcc\xa7\xcc\xa4\xcc\xa4\xcc\xae\xcc\x9f\xcc\xae\xcc\xbb\xcc\x9f\xcc\xaa\xcc\xb1\xcc\xac\xcd\x8e\xcc\x9f\xcc\x99\xcc\x9d\xcd\x94\xcd\x99\xcc\xb2\xcc\x8e\xcc\x92\xcc\x85\xcc\x94\xcc\x89\xcd\x97\xcc\x88\xcc\x81\xcd\x9b\xcd\x8a\xcc\x88\xcd\x86\xcc\x80\xcc\x88\xcc\x81\xcd\x8a\xcd\x91\xcc\x8e\xcc\x90\xcc\x8c\xcc\x92\xcd\x86\xcd\x8a\xcc\x95\xcc\x95\xcc\x9a".to_vec();
        let expected: Result<Operation, OperationParseError> = Ok(Operation::del(key_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn put_to_string_works() {
        let key_bytes: Vec<u8> = b"MyKey".to_vec();
        let value_bytes: Vec<u8> = b"MyValue".to_vec();
        let put_operation: Operation = Operation::put(key_bytes, value_bytes);

        let actual: String = put_operation.to_string();
        let expected: &str = "Put\r\n5\r\nMyKey\r\n7\r\nMyValue\r\n";

        assert_eq!(actual, expected);
    }

    #[test]
    fn get_to_string_works() {
        let key_bytes: Vec<u8> = b"12345".to_vec();
        let get_operation: Operation = Operation::get(key_bytes);

        let actual: String = get_operation.to_string();
        let expected: &str = "Get\r\n5\r\n12345\r\n";

        assert_eq!(actual, expected);
    }

    #[test]
    fn del_to_string_works() {
        let key_bytes: Vec<u8> = b"DeleteMyDataNow.Com".to_vec();
        let del_operation: Operation = Operation::del(key_bytes);

        let actual: String = del_operation.to_string();
        let expected: &str = "Del\r\n19\r\nDeleteMyDataNow.Com\r\n";

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_empty_bytes() {
        let mut input: &[u8] = b"";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Err(WireFormatParseError::InvalidTypeEncoding);
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_unknown_prefix() {
        let mut input: &[u8] = b"unknown\r\n";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Err(WireFormatParseError::UnknownType("unknown".to_owned()));
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_unknown_type_carries_the_bad_type() {
        let mut input: &[u8] = b"notacommand\r\n";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Err(WireFormatParseError::UnknownType("notacommand".to_owned()));
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_cmd_put_valid() {
        let mut input: &[u8] = b"op\r\nPut\r\n6\r\nKey123\r\n7\r\nValue12\r\n";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Ok(WireFormat::cmd(Operation::put(
            b"Key123".to_vec(),
            b"Value12".to_vec(),
        )));
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_cmd_get_valid() {
        let mut input: &[u8] = b"op\r\nGet\r\n5\r\nMyKey\r\n";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Ok(WireFormat::cmd(Operation::get(b"MyKey".to_vec())));
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_cmd_del_valid() {
        let mut input: &[u8] = b"op\r\nDel\r\n4\r\nTree\r\n";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Ok(WireFormat::cmd(Operation::del(b"Tree".to_vec())));
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_cmd_bad_operation() {
        let mut input: &[u8] = b"op\r\nInvalid\r\n";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Err(WireFormatParseError::InvalidCmdEncoding(
            OperationParseError::UnknownOperation("Invalid".to_owned()),
        ));
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_cmd_bad_key_len() {
        let mut input: &[u8] = b"op\r\nPut\r\nNotANumber\r\n";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Err(WireFormatParseError::InvalidCmdEncoding(
            OperationParseError::InvalidKeyLenEncoding,
        ));
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_simple_string_valid() {
        let mut input: &[u8] = b"sstr\r\nHello World\r\n";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Ok(WireFormat::simple_string("Hello World".to_string()));
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_simple_string_empty() {
        let mut input: &[u8] = b"sstr\r\n\r\n";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Ok(WireFormat::simple_string("".to_string()));
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_wire_format_simple_string_missing_terminator() {
        let mut input: &[u8] = b"sstr\r\nHello";
        let actual = WireFormat::from_reader(&mut input);
        let expected = Err(WireFormatParseError::InvalidSimpleStringEncoding);
        assert_eq!(actual, expected);
    }

    #[test]
    fn wire_format_cmd_put_to_string() {
        let wf = WireFormat::cmd(Operation::put(b"MyKey".to_vec(), b"MyValue".to_vec()));
        let actual = wf.to_string();
        let expected = "op\r\nPut\r\n5\r\nMyKey\r\n7\r\nMyValue\r\n";
        assert_eq!(actual, expected);
    }

    #[test]
    fn wire_format_cmd_get_to_string() {
        let wf = WireFormat::cmd(Operation::get(b"MyKey".to_vec()));
        let actual = wf.to_string();
        let expected = "op\r\nGet\r\n5\r\nMyKey\r\n";
        assert_eq!(actual, expected);
    }

    #[test]
    fn wire_format_cmd_del_to_string() {
        let wf = WireFormat::cmd(Operation::del(b"MyKey".to_vec()));
        let actual = wf.to_string();
        let expected = "op\r\nDel\r\n5\r\nMyKey\r\n";
        assert_eq!(actual, expected);
    }

    #[test]
    fn wire_format_simple_string_to_string() {
        let wf = WireFormat::simple_string("OK".to_string());
        let actual = wf.to_string();
        let expected = "sstr\r\nOK\r\n";
        assert_eq!(actual, expected);
    }

    #[test]
    fn wire_format_cmd_to_string_back_to_wire_format() {
        let wf = WireFormat::cmd(Operation::put(b"MyKey".to_vec(), b"MyValue".to_vec()));
        let bytes = wf.to_string().into_bytes();
        let mut reader: &[u8] = &bytes;
        let wf_back = WireFormat::from_reader(&mut reader).expect("wire format bytes were valid");
        assert_eq!(wf, wf_back);
    }

    #[test]
    fn wire_format_simple_string_to_string_back_to_wire_format() {
        let wf = WireFormat::simple_string("Hello World".to_string());
        let bytes = wf.to_string().into_bytes();
        let mut reader: &[u8] = &bytes;
        let wf_back = WireFormat::from_reader(&mut reader).expect("wire format bytes were valid");
        assert_eq!(wf, wf_back);
    }

    #[test]
    fn from_reader_for_operation_get_key_contains_crlf() {
        let mut wire_bytes: &[u8] = b"Get\r\n8\r\nfoo\r\nbar\r\n";
        let actual = Operation::from_reader(&mut wire_bytes);

        let key_bytes: Vec<u8> = b"foo\r\nbar".to_vec();
        let expected: Result<Operation, OperationParseError> = Ok(Operation::get(key_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_operation_put_value_contains_crlf() {
        let mut wire_bytes: &[u8] = b"Put\r\n1\r\nk\r\n4\r\nv\r\nw\r\n";
        let actual = Operation::from_reader(&mut wire_bytes);

        let key_bytes: Vec<u8> = b"k".to_vec();
        let value_bytes: Vec<u8> = b"v\r\nw".to_vec();
        let expected: Result<Operation, OperationParseError> =
            Ok(Operation::put(key_bytes, value_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn operation_into_bytes_roundtrip_put() {
        let original = Operation::put(b"MyKey".to_vec(), b"MyValue".to_vec());
        let bytes = original.clone().into_bytes();
        let roundtripped = Operation::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn operation_into_bytes_roundtrip_get() {
        let original = Operation::get(b"MyKey".to_vec());
        let bytes = original.clone().into_bytes();
        let roundtripped = Operation::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn operation_into_bytes_roundtrip_del() {
        let original = Operation::del(b"MyKey".to_vec());
        let bytes = original.clone().into_bytes();
        let roundtripped = Operation::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn operation_into_bytes_roundtrip_put_non_utf8_value() {
        let original = Operation::put(b"key".to_vec(), vec![0xFF, 0xFE, 0x00, 0xC3, 0x28]);
        let bytes = original.clone().into_bytes();
        let roundtripped = Operation::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn operation_into_bytes_roundtrip_get_non_utf8_key() {
        let original = Operation::get(vec![0xFF, 0xFE, 0x00, 0xC3, 0x28]);
        let bytes = original.clone().into_bytes();
        let roundtripped = Operation::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn operation_into_bytes_roundtrip_del_non_utf8_key() {
        let original = Operation::del(vec![0xFF, 0xFE, 0x00, 0xC3, 0x28]);
        let bytes = original.clone().into_bytes();
        let roundtripped = Operation::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn wire_format_into_bytes_roundtrip_cmd_non_utf8() {
        let original = WireFormat::cmd(Operation::put(
            vec![0xFF, 0xFE, 0x00],
            vec![0xC3, 0x28, 0xFF],
        ));
        let bytes = original.clone().into_bytes();
        let roundtripped = WireFormat::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn wire_format_into_bytes_roundtrip_sstr() {
        let original = WireFormat::simple_string("Hello World".to_string());
        let bytes = original.clone().into_bytes();
        let roundtripped = WireFormat::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn operation_to_string_back_to_operation_is_lossy_for_non_utf8_bytes() {
        let original = Operation::get(vec![0xFF]);
        let bytes = original.to_string().into_bytes();
        let mut reader: &[u8] = &bytes;
        let roundtripped = Operation::from_reader(&mut reader)
            .expect("parser succeeds but produces corrupted bytes");

        assert_ne!(original, roundtripped);
    }
}
