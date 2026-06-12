use std::{
    fmt,
    io::{BufRead, Read, Write},
    num::ParseIntError,
};

use strum::VariantNames;

pub struct KvRequest {
    pub command: KvCommand,
    // TODO: add some sort of top-level metadata here, or collapse into just KvCommand
    // fields could be like request id, trace context, etc etc
}

const MAX_KEY_LEN: usize = 64 * 1024;
const MAX_VALUE_LEN: usize = 8 * 1024 * 1024;
const CRLF: &[u8; 2] = b"\r\n";

#[derive(Debug, thiserror::Error)]
pub enum KvRequestError {
    #[error("expected to find a CRLF when parsing at {0}")]
    MissingCrlf(String),
    #[error("expected one of: {variants:?}, but got: {0}", variants = KvCommand::VARIANTS)]
    BadOperation(String),
    #[error("expected to parse valid utf-8 bytes")]
    BadUtf8(#[from] std::str::Utf8Error),
    #[error("io error parsing the request")]
    IoError(#[from] std::io::Error),
    #[error("keys and values should be valid usizes")]
    NonIntLength(#[from] ParseIntError),
    #[error("key length {0} exceeds maximum of {max}", max = MAX_KEY_LEN)]
    KeyTooLarge(usize),
    #[error("value length {0} exceeds maximum of {max}", max = MAX_VALUE_LEN)]
    ValueTooLarge(usize),
    #[error("request ended before the full payload arrived")]
    Truncated,
}

#[derive(Debug, Clone, PartialEq, strum::VariantNames, strum::IntoStaticStr)]
#[strum(serialize_all = "PascalCase")]
pub enum KvCommand {
    Put(Vec<u8>, Vec<u8>),
    Get(Vec<u8>),
    Del(Vec<u8>),
}

impl KvRequest {
    pub fn from_reader<T: BufRead>(reader: &mut T) -> Result<KvRequest, KvRequestError> {
        Ok(Self {
            command: KvCommand::from_reader(reader)?,
        })
    }
}

impl KvCommand {
    fn name(&self) -> &'static str {
        self.into()
    }

    pub fn write_to<W: Write>(&self, w: &mut W) -> Result<(), std::io::Error> {
        w.write_all(self.name().as_bytes())?;
        w.write_all(CRLF)?;

        match self {
            Self::Get(key) | Self::Del(key) => {
                w.write_all(key.len().to_string().as_bytes())?;
                w.write_all(CRLF)?;
                w.write_all(key)?;
            }
            Self::Put(key, value) => {
                w.write_all(key.len().to_string().as_bytes())?;
                w.write_all(CRLF)?;
                w.write_all(key)?;
                w.write_all(CRLF)?;
                w.write_all(value.len().to_string().as_bytes())?;
                w.write_all(CRLF)?;
                w.write_all(value)?;
            }
        }

        w.write_all(CRLF)?;

        Ok(())
    }

    pub fn from_reader<T: BufRead>(reader: &mut T) -> Result<KvCommand, KvRequestError> {
        let mut op_bytes: Vec<u8> = Vec::new();
        reader.read_until(b'\n', &mut op_bytes)?;

        let op_bytes_trimmed = op_bytes
            .strip_suffix(CRLF)
            .ok_or(KvRequestError::MissingCrlf("after operation".to_owned()))?;

        let op_str: &str = std::str::from_utf8(op_bytes_trimmed)?;

        if !KvCommand::VARIANTS.contains(&op_str) {
            return Err(KvRequestError::BadOperation(op_str.to_owned()));
        }

        let mut key_length_bytes: Vec<u8> = Vec::new();
        reader.read_until(b'\n', &mut key_length_bytes)?;

        let key_length_bytes_trimmed = key_length_bytes
            .strip_suffix(CRLF)
            .ok_or(KvRequestError::MissingCrlf("after key length".to_owned()))?;

        let key_length: usize = std::str::from_utf8(key_length_bytes_trimmed)?.parse()?;

        if key_length > MAX_KEY_LEN {
            return Err(KvRequestError::KeyTooLarge(key_length));
        }

        let mut key_bytes: Vec<u8> = Vec::new();
        let read = reader
            .by_ref()
            .take(key_length as u64)
            .read_to_end(&mut key_bytes)?;
        if read != key_length {
            return Err(KvRequestError::Truncated);
        }

        expect_crlf(reader, "after key")?;

        match op_str {
            "Get" => Ok(KvCommand::Get(key_bytes)),
            "Del" => Ok(KvCommand::Del(key_bytes)),
            "Put" => {
                let mut value_length_bytes: Vec<u8> = Vec::new();
                reader.read_until(b'\n', &mut value_length_bytes)?;

                let value_length_bytes_trimmed = value_length_bytes
                    .strip_suffix(CRLF)
                    .ok_or(KvRequestError::MissingCrlf("after value length".to_owned()))?;
                let value_length: usize =
                    std::str::from_utf8(value_length_bytes_trimmed)?.parse()?;

                if value_length > MAX_VALUE_LEN {
                    return Err(KvRequestError::ValueTooLarge(value_length));
                }

                let mut value_bytes: Vec<u8> = Vec::new();
                let read = reader
                    .by_ref()
                    .take(value_length as u64)
                    .read_to_end(&mut value_bytes)?;
                if read != value_length {
                    return Err(KvRequestError::Truncated);
                }

                expect_crlf(reader, "after put value")?;

                Ok(KvCommand::Put(key_bytes, value_bytes))
            }
            _ => unreachable!("all variants of Operation were matched as strs during byte parsing"),
        }
    }
}

fn expect_crlf<R: Read>(reader: &mut R, context: &str) -> Result<(), KvRequestError> {
    let mut buf: [u8; 2] = [0; 2];
    reader.read_exact(&mut buf)?;
    if &buf != CRLF {
        return Err(KvRequestError::MissingCrlf(context.to_owned()));
    }
    Ok(())
}

impl fmt::Display for KvCommand {
    // Lossy: non-UTF8 bytes are replaced with U+FFFD. Do not use for round-tripping (KvCommand -> Display/ToString -> KvCommand).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ", self.name())?;

        match self {
            Self::Del(key) | Self::Get(key) => {
                let key_lossy = String::from_utf8_lossy(key);
                write!(f, "key={key_lossy}")
            }
            Self::Put(key, value) => {
                let key_lossy = String::from_utf8_lossy(key);
                let value_lossy = String::from_utf8_lossy(value);
                write!(f, "key={key_lossy} value={value_lossy}")
            }
        }
    }
}

impl fmt::Display for KvRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_reader_for_kv_command_bad_operation_bytes() {
        let mut bad_bytes: &[u8] = b"hello!\r\n".as_slice();
        let actual = KvCommand::from_reader(&mut bad_bytes);
        assert!(matches!(actual, Err(KvRequestError::BadOperation(op)) if op == "hello!"));
    }

    #[test]
    fn from_reader_for_kv_command_empty_byte_arr() {
        let mut empty_byte_arr: &[u8] = b"".as_slice();
        let actual = KvCommand::from_reader(&mut empty_byte_arr);
        assert!(matches!(actual, Err(KvRequestError::MissingCrlf(_))));
    }

    #[test]
    fn from_reader_for_kv_command_bad_operator() {
        let mut byte_arr: &[u8] = b"InvalidOperation\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(
            matches!(actual, Err(KvRequestError::BadOperation(op)) if op == "InvalidOperation")
        );
    }

    #[test]
    fn from_reader_for_kv_command_bad_key_len() {
        let mut byte_arr: &[u8] = b"Put\r\nHello\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(matches!(actual, Err(KvRequestError::NonIntLength(_))));
    }

    #[test]
    fn from_reader_for_kv_command_mismatch_key_len() {
        let mut byte_arr: &[u8] = b"Get\r\n5\r\nNotFive\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(matches!(actual, Err(KvRequestError::MissingCrlf(_))));
    }

    #[test]
    fn from_reader_for_kv_command_bad_value_len_encoding() {
        let mut byte_arr: &[u8] = b"Put\r\n5\r\n12345\r\nInvalidLen";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(matches!(actual, Err(KvRequestError::MissingCrlf(_))));
    }

    #[test]
    fn from_reader_for_kv_command_value_len_mismatch() {
        let mut byte_arr: &[u8] = b"Put\r\n5\r\n12345\r\n6\r\nSeven";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(matches!(actual, Err(KvRequestError::Truncated)));
    }

    #[test]
    fn from_reader_for_kv_command_key_too_large() {
        let mut byte_arr: &[u8] = b"Get\r\n70000\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(matches!(actual, Err(KvRequestError::KeyTooLarge(70000))));
    }

    #[test]
    fn from_reader_for_kv_command_key_truncated() {
        let mut byte_arr: &[u8] = b"Get\r\n10\r\nabc";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(matches!(actual, Err(KvRequestError::Truncated)));
    }

    #[test]
    fn from_reader_for_kv_command_value_too_large() {
        let mut byte_arr: &[u8] = b"Put\r\n3\r\nabc\r\n9999999999\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(matches!(actual, Err(KvRequestError::ValueTooLarge(_))));
    }

    #[test]
    fn from_reader_key_length_exactly_at_cap_succeeds() {
        // The cap check is `>`, so a key of exactly MAX_KEY_LEN must be accepted.
        let key = vec![b'x'; MAX_KEY_LEN];
        let mut wire = format!("Get\r\n{}\r\n", MAX_KEY_LEN).into_bytes();
        wire.extend_from_slice(&key);
        wire.extend_from_slice(CRLF);

        let mut reader: &[u8] = &wire;
        let actual = KvCommand::from_reader(&mut reader);
        assert_eq!(actual.unwrap(), KvCommand::Get(key));
    }

    #[test]
    fn from_reader_key_length_one_over_cap_fails() {
        let wire = format!("Get\r\n{}\r\n", MAX_KEY_LEN + 1).into_bytes();
        let mut reader: &[u8] = &wire;
        let actual = KvCommand::from_reader(&mut reader);
        assert!(matches!(actual, Err(KvRequestError::KeyTooLarge(n)) if n == MAX_KEY_LEN + 1));
    }

    #[test]
    fn from_reader_value_length_one_over_cap_fails() {
        // The cap is checked before any payload is read, so no value bytes needed.
        let wire = format!("Put\r\n3\r\nabc\r\n{}\r\n", MAX_VALUE_LEN + 1).into_bytes();
        let mut reader: &[u8] = &wire;
        let actual = KvCommand::from_reader(&mut reader);
        assert!(matches!(actual, Err(KvRequestError::ValueTooLarge(n)) if n == MAX_VALUE_LEN + 1));
    }

    #[test]
    fn from_reader_for_kv_command_non_utf8_operation() {
        let mut byte_arr: &[u8] = b"\xff\xfe\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(matches!(actual, Err(KvRequestError::BadUtf8(_))));
    }

    #[test]
    fn from_reader_for_kv_command_non_utf8_key_length() {
        let mut byte_arr: &[u8] = b"Put\r\n\xff\xfe\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(matches!(actual, Err(KvRequestError::BadUtf8(_))));
    }

    #[test]
    fn from_reader_for_kv_command_non_utf8_value_length() {
        let mut byte_arr: &[u8] = b"Put\r\n3\r\nabc\r\n\xff\xfe\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert!(matches!(actual, Err(KvRequestError::BadUtf8(_))));
    }

    #[test]
    fn from_reader_for_kv_command_put_valid() {
        let mut byte_arr: &[u8] = b"Put\r\n6\r\nKey123\r\n7\r\nValue12\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert_eq!(
            actual.unwrap(),
            KvCommand::Put(b"Key123".to_vec(), b"Value12".to_vec())
        );
    }

    #[test]
    fn from_reader_for_kv_command_get_valid() {
        let mut byte_arr: &[u8] = b"Get\r\n17\r\nDrakeIsABadArtist\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert_eq!(
            actual.unwrap(),
            KvCommand::Get(b"DrakeIsABadArtist".to_vec())
        );
    }

    #[test]
    fn from_reader_for_kv_command_del_valid() {
        let mut byte_arr: &[u8] = b"Del\r\n4\r\nTree\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);
        assert_eq!(actual.unwrap(), KvCommand::Del(b"Tree".to_vec()));
    }

    #[test]
    fn from_reader_for_non_byte_safe_string() {
        // 𝕳𝖊𝖑𝖑𝖔 Wörld! ñoño 日本語 中文 한국어 العربية עברית ℃ ™ © ® € £ ¥ ✓ ← ↑ → ↓ ♠ ♣ ♥ ♦
        let mut byte_arr: &[u8] = b"Put\r\n5\r\nMyKey\r\n148\r\n\xf0\x9d\x95\xb3\xf0\x9d\x96\x8a\xf0\x9d\x96\x91\xf0\x9d\x96\x91\xf0\x9d\x96\x94 W\xc3\xb6rld! \xc3\xb1o\xc3\xb1o \xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e \xe4\xb8\xad\xe6\x96\x87 \xed\x95\x9c\xea\xb5\xad\xec\x96\xb4 \xd8\xa7\xd9\x84\xd8\xb9\xd8\xb1\xd8\xa8\xd9\x8a\xd8\xa9 \xd7\xa2\xd7\x91\xd7\xa8\xd7\x99\xd7\xaa \xe2\x84\x83 \xe2\x84\xa2 \xc2\xa9 \xc2\xae \xe2\x82\xac \xc2\xa3 \xc2\xa5 \xe2\x9c\x93 \xe2\x86\x90 \xe2\x86\x91 \xe2\x86\x92 \xe2\x86\x93 \xe2\x99\xa0 \xe2\x99\xa3 \xe2\x99\xa5 \xe2\x99\xa6\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);

        let key_bytes: Vec<u8> = b"MyKey".to_vec();
        let value_bytes: Vec<u8> = b"\xf0\x9d\x95\xb3\xf0\x9d\x96\x8a\xf0\x9d\x96\x91\xf0\x9d\x96\x91\xf0\x9d\x96\x94 W\xc3\xb6rld! \xc3\xb1o\xc3\xb1o \xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e \xe4\xb8\xad\xe6\x96\x87 \xed\x95\x9c\xea\xb5\xad\xec\x96\xb4 \xd8\xa7\xd9\x84\xd8\xb9\xd8\xb1\xd8\xa8\xd9\x8a\xd8\xa9 \xd7\xa2\xd7\x91\xd7\xa8\xd7\x99\xd7\xaa \xe2\x84\x83 \xe2\x84\xa2 \xc2\xa9 \xc2\xae \xe2\x82\xac \xc2\xa3 \xc2\xa5 \xe2\x9c\x93 \xe2\x86\x90 \xe2\x86\x91 \xe2\x86\x92 \xe2\x86\x93 \xe2\x99\xa0 \xe2\x99\xa3 \xe2\x99\xa5 \xe2\x99\xa6".to_vec();
        assert_eq!(actual.unwrap(), KvCommand::Put(key_bytes, value_bytes));
    }

    #[test]
    fn from_reader_for_kv_command_zalgo_key_del() {
        // Z̸̡̢̛̛̺̙͔̮͍̺̘̣̺̖͚̬̖͙͍̣̘̤̟̪̦̬͕̖̩̠͕͖̤̟̱̙̼̳͙̬̦̳͉̦̻̙̥̗̘͇͍̤̫̫͎̱̰͈̺̜̤͔̀͐͌̀͂͗̈́̌̅̊͑̋̒͒͊̀̓̏͊͌̎̈́̀̈́͘͘͜͠͝͠͝ͅͅą̷̧̧̢̛̛̛̹̟͎͉̝̩̬͚͖̝̩̱̩͕͔̖͇̘͇̗̯̙̣͙̮̙̗̹̺͕̱̰̱̲̬̞̤̳̹͍̝͕͑͒̒̐̓̐̃̃̽̏̾̆̋͌͌̒́̋̅́̏͒͗̎̒̔̑̀͑̿̄͑͑̿̈́͋̕̕͜͠͠ͅl̴͇̜̣̬̝̮̭̟͇͚̖͈͎͚͕͔͕͚̹̺̲̙̺̹͂͌̓̂̀̾̅̀̉̒̃̑̋̅͘̕͜͝g̷͉̲̙̥̜̟͔͓̰̯͇̮͎͓̈͛̒̊͐͛̃̊̆̂̎̈́͌͆̀͊̌̃̍̃̿͝͝ͅo̷̧̤̤̮̟̮̻̟̪̱̬͎̟̙̝͔͙̲̎̒̅̔̉͗̈́͛͊̈͆̀̈́͊͑̎̐̌̒͆͊̕̕̚
        let mut byte_arr: &[u8] = b"Del\r\n644\r\n\x5a\xcc\xb8\xcc\xa1\xcc\xa2\xcc\x9b\xcc\x9b\xcc\xba\xcc\x99\xcd\x94\xcc\xae\xcd\x8d\xcc\xba\xcc\x98\xcc\xa3\xcc\xba\xcc\x96\xcd\x9a\xcc\xac\xcc\x96\xcd\x99\xcd\x8d\xcc\xa3\xcc\x98\xcc\xa4\xcc\x9f\xcc\xaa\xcc\xa6\xcc\xac\xcd\x95\xcc\x96\xcc\xa9\xcc\xa0\xcd\x95\xcd\x96\xcc\xa4\xcc\x9f\xcc\xb1\xcc\x99\xcc\xbc\xcc\xb3\xcd\x99\xcc\xac\xcc\xa6\xcc\xb3\xcd\x89\xcc\xa6\xcc\xbb\xcc\x99\xcc\xa5\xcc\x97\xcc\x98\xcd\x87\xcd\x8d\xcc\xa4\xcc\xab\xcc\xab\xcd\x8e\xcc\xb1\xcc\xb0\xcd\x88\xcc\xba\xcc\x9c\xcc\xa4\xcd\x94\xcc\x80\xcd\x90\xcd\x8c\xcc\x80\xcd\x82\xcd\x97\xcc\x88\xcc\x81\xcc\x8c\xcc\x85\xcc\x8a\xcd\x91\xcc\x8b\xcc\x92\xcd\x92\xcd\x8a\xcc\x80\xcc\x93\xcc\x8f\xcd\x8a\xcd\x8c\xcc\x8e\xcc\x88\xcc\x81\xcc\x80\xcc\x88\xcc\x81\xcd\x98\xcd\x98\xcd\x9c\xcd\xa0\xcd\x9d\xcd\xa0\xcd\x9d\xcd\x85\xcd\x85\xc4\x85\xcc\xb7\xcc\xa7\xcc\xa7\xcc\xa2\xcc\x9b\xcc\x9b\xcc\x9b\xcc\xb9\xcc\x9f\xcd\x8e\xcd\x89\xcc\x9d\xcc\xa9\xcc\xac\xcd\x9a\xcd\x96\xcc\x9d\xcc\xa9\xcc\xb1\xcc\xa9\xcd\x95\xcd\x94\xcc\x96\xcd\x87\xcc\x98\xcd\x87\xcc\x97\xcc\xaf\xcc\x99\xcc\xa3\xcd\x99\xcc\xae\xcc\x99\xcc\x97\xcc\xb9\xcc\xba\xcd\x95\xcc\xb1\xcc\xb0\xcc\xb1\xcc\xb2\xcc\xac\xcc\x9e\xcc\xa4\xcc\xb3\xcc\xb9\xcd\x8d\xcc\x9d\xcd\x95\xcd\x91\xcd\x92\xcc\x92\xcc\x90\xcc\x93\xcc\x90\xcc\x83\xcc\x83\xcc\xbd\xcc\x8f\xcc\xbe\xcc\x86\xcc\x8b\xcd\x8c\xcd\x8c\xcc\x92\xcc\x81\xcc\x8b\xcc\x85\xcc\x81\xcc\x8f\xcd\x92\xcd\x97\xcc\x8e\xcc\x92\xcc\x94\xcc\x91\xcc\x80\xcd\x91\xcc\xbf\xcc\x84\xcd\x91\xcd\x91\xcc\xbf\xcc\x88\xcc\x81\xcd\x8b\xcc\x95\xcc\x95\xcd\x9c\xcd\xa0\xcd\xa0\xcd\x85\x6c\xcc\xb4\xcd\x87\xcc\x9c\xcc\xa3\xcc\xac\xcc\x9d\xcc\xae\xcc\xad\xcc\x9f\xcd\x87\xcd\x9a\xcc\x96\xcd\x88\xcd\x8e\xcd\x9a\xcd\x95\xcd\x94\xcd\x95\xcd\x9a\xcc\xb9\xcc\xba\xcc\xb2\xcc\x99\xcc\xba\xcc\xb9\xcd\x82\xcd\x8c\xcc\x93\xcc\x82\xcc\x80\xcc\xbe\xcc\x85\xcc\x80\xcc\x89\xcc\x92\xcc\x83\xcc\x91\xcc\x8b\xcc\x85\xcd\x98\xcc\x95\xcd\x9c\xcd\x9d\x67\xcc\xb7\xcd\x89\xcc\xb2\xcc\x99\xcc\xa5\xcc\x9c\xcc\x9f\xcd\x94\xcd\x93\xcc\xb0\xcc\xaf\xcd\x87\xcc\xae\xcd\x8e\xcd\x93\xcc\x88\xcd\x9b\xcc\x92\xcc\x8a\xcd\x90\xcd\x9b\xcc\x83\xcc\x8a\xcc\x86\xcc\x82\xcc\x8e\xcc\x88\xcc\x81\xcd\x8c\xcd\x86\xcc\x80\xcd\x8a\xcc\x8c\xcc\x83\xcc\x8d\xcc\x83\xcc\xbf\xcd\x9d\xcd\x9d\xcd\x85\x6f\xcc\xb7\xcc\xa7\xcc\xa4\xcc\xa4\xcc\xae\xcc\x9f\xcc\xae\xcc\xbb\xcc\x9f\xcc\xaa\xcc\xb1\xcc\xac\xcd\x8e\xcc\x9f\xcc\x99\xcc\x9d\xcd\x94\xcd\x99\xcc\xb2\xcc\x8e\xcc\x92\xcc\x85\xcc\x94\xcc\x89\xcd\x97\xcc\x88\xcc\x81\xcd\x9b\xcd\x8a\xcc\x88\xcd\x86\xcc\x80\xcc\x88\xcc\x81\xcd\x8a\xcd\x91\xcc\x8e\xcc\x90\xcc\x8c\xcc\x92\xcd\x86\xcd\x8a\xcc\x95\xcc\x95\xcc\x9a\r\n";
        let actual = KvCommand::from_reader(&mut byte_arr);

        let key_bytes: Vec<u8> = b"\x5a\xcc\xb8\xcc\xa1\xcc\xa2\xcc\x9b\xcc\x9b\xcc\xba\xcc\x99\xcd\x94\xcc\xae\xcd\x8d\xcc\xba\xcc\x98\xcc\xa3\xcc\xba\xcc\x96\xcd\x9a\xcc\xac\xcc\x96\xcd\x99\xcd\x8d\xcc\xa3\xcc\x98\xcc\xa4\xcc\x9f\xcc\xaa\xcc\xa6\xcc\xac\xcd\x95\xcc\x96\xcc\xa9\xcc\xa0\xcd\x95\xcd\x96\xcc\xa4\xcc\x9f\xcc\xb1\xcc\x99\xcc\xbc\xcc\xb3\xcd\x99\xcc\xac\xcc\xa6\xcc\xb3\xcd\x89\xcc\xa6\xcc\xbb\xcc\x99\xcc\xa5\xcc\x97\xcc\x98\xcd\x87\xcd\x8d\xcc\xa4\xcc\xab\xcc\xab\xcd\x8e\xcc\xb1\xcc\xb0\xcd\x88\xcc\xba\xcc\x9c\xcc\xa4\xcd\x94\xcc\x80\xcd\x90\xcd\x8c\xcc\x80\xcd\x82\xcd\x97\xcc\x88\xcc\x81\xcc\x8c\xcc\x85\xcc\x8a\xcd\x91\xcc\x8b\xcc\x92\xcd\x92\xcd\x8a\xcc\x80\xcc\x93\xcc\x8f\xcd\x8a\xcd\x8c\xcc\x8e\xcc\x88\xcc\x81\xcc\x80\xcc\x88\xcc\x81\xcd\x98\xcd\x98\xcd\x9c\xcd\xa0\xcd\x9d\xcd\xa0\xcd\x9d\xcd\x85\xcd\x85\xc4\x85\xcc\xb7\xcc\xa7\xcc\xa7\xcc\xa2\xcc\x9b\xcc\x9b\xcc\x9b\xcc\xb9\xcc\x9f\xcd\x8e\xcd\x89\xcc\x9d\xcc\xa9\xcc\xac\xcd\x9a\xcd\x96\xcc\x9d\xcc\xa9\xcc\xb1\xcc\xa9\xcd\x95\xcd\x94\xcc\x96\xcd\x87\xcc\x98\xcd\x87\xcc\x97\xcc\xaf\xcc\x99\xcc\xa3\xcd\x99\xcc\xae\xcc\x99\xcc\x97\xcc\xb9\xcc\xba\xcd\x95\xcc\xb1\xcc\xb0\xcc\xb1\xcc\xb2\xcc\xac\xcc\x9e\xcc\xa4\xcc\xb3\xcc\xb9\xcd\x8d\xcc\x9d\xcd\x95\xcd\x91\xcd\x92\xcc\x92\xcc\x90\xcc\x93\xcc\x90\xcc\x83\xcc\x83\xcc\xbd\xcc\x8f\xcc\xbe\xcc\x86\xcc\x8b\xcd\x8c\xcd\x8c\xcc\x92\xcc\x81\xcc\x8b\xcc\x85\xcc\x81\xcc\x8f\xcd\x92\xcd\x97\xcc\x8e\xcc\x92\xcc\x94\xcc\x91\xcc\x80\xcd\x91\xcc\xbf\xcc\x84\xcd\x91\xcd\x91\xcc\xbf\xcc\x88\xcc\x81\xcd\x8b\xcc\x95\xcc\x95\xcd\x9c\xcd\xa0\xcd\xa0\xcd\x85\x6c\xcc\xb4\xcd\x87\xcc\x9c\xcc\xa3\xcc\xac\xcc\x9d\xcc\xae\xcc\xad\xcc\x9f\xcd\x87\xcd\x9a\xcc\x96\xcd\x88\xcd\x8e\xcd\x9a\xcd\x95\xcd\x94\xcd\x95\xcd\x9a\xcc\xb9\xcc\xba\xcc\xb2\xcc\x99\xcc\xba\xcc\xb9\xcd\x82\xcd\x8c\xcc\x93\xcc\x82\xcc\x80\xcc\xbe\xcc\x85\xcc\x80\xcc\x89\xcc\x92\xcc\x83\xcc\x91\xcc\x8b\xcc\x85\xcd\x98\xcc\x95\xcd\x9c\xcd\x9d\x67\xcc\xb7\xcd\x89\xcc\xb2\xcc\x99\xcc\xa5\xcc\x9c\xcc\x9f\xcd\x94\xcd\x93\xcc\xb0\xcc\xaf\xcd\x87\xcc\xae\xcd\x8e\xcd\x93\xcc\x88\xcd\x9b\xcc\x92\xcc\x8a\xcd\x90\xcd\x9b\xcc\x83\xcc\x8a\xcc\x86\xcc\x82\xcc\x8e\xcc\x88\xcc\x81\xcd\x8c\xcd\x86\xcc\x80\xcd\x8a\xcc\x8c\xcc\x83\xcc\x8d\xcc\x83\xcc\xbf\xcd\x9d\xcd\x9d\xcd\x85\x6f\xcc\xb7\xcc\xa7\xcc\xa4\xcc\xa4\xcc\xae\xcc\x9f\xcc\xae\xcc\xbb\xcc\x9f\xcc\xaa\xcc\xb1\xcc\xac\xcd\x8e\xcc\x9f\xcc\x99\xcc\x9d\xcd\x94\xcd\x99\xcc\xb2\xcc\x8e\xcc\x92\xcc\x85\xcc\x94\xcc\x89\xcd\x97\xcc\x88\xcc\x81\xcd\x9b\xcd\x8a\xcc\x88\xcd\x86\xcc\x80\xcc\x88\xcc\x81\xcd\x8a\xcd\x91\xcc\x8e\xcc\x90\xcc\x8c\xcc\x92\xcd\x86\xcd\x8a\xcc\x95\xcc\x95\xcc\x9a".to_vec();
        assert_eq!(actual.unwrap(), KvCommand::Del(key_bytes));
    }

    #[test]
    fn put_to_string_works() {
        let cmd = KvCommand::Put(b"MyKey".to_vec(), b"MyValue".to_vec());
        let actual = cmd.to_string();
        let expected = "Put key=MyKey value=MyValue";
        assert_eq!(actual, expected);
    }

    #[test]
    fn get_to_string_works() {
        let cmd = KvCommand::Get(b"12345".to_vec());
        let actual = cmd.to_string();
        let expected = "Get key=12345";
        assert_eq!(actual, expected);
    }

    #[test]
    fn del_to_string_works() {
        let cmd = KvCommand::Del(b"DeleteMyDataNow.Com".to_vec());
        let actual = cmd.to_string();
        let expected = "Del key=DeleteMyDataNow.Com";
        assert_eq!(actual, expected);
    }

    #[test]
    fn from_reader_for_kv_command_get_key_contains_crlf() {
        let mut wire_bytes: &[u8] = b"Get\r\n8\r\nfoo\r\nbar\r\n";
        let actual = KvCommand::from_reader(&mut wire_bytes);
        assert_eq!(actual.unwrap(), KvCommand::Get(b"foo\r\nbar".to_vec()));
    }

    #[test]
    fn from_reader_for_kv_command_put_value_contains_crlf() {
        let mut wire_bytes: &[u8] = b"Put\r\n1\r\nk\r\n4\r\nv\r\nw\r\n";
        let actual = KvCommand::from_reader(&mut wire_bytes);
        assert_eq!(
            actual.unwrap(),
            KvCommand::Put(b"k".to_vec(), b"v\r\nw".to_vec())
        );
    }

    #[test]
    fn kv_command_write_to_roundtrip_put() {
        let original = KvCommand::Put(b"MyKey".to_vec(), b"MyValue".to_vec());
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvCommand::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn kv_command_write_to_roundtrip_get() {
        let original = KvCommand::Get(b"MyKey".to_vec());
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvCommand::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn kv_command_write_to_roundtrip_del() {
        let original = KvCommand::Del(b"MyKey".to_vec());
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvCommand::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn kv_command_write_to_roundtrip_put_non_utf8_value() {
        let original = KvCommand::Put(b"key".to_vec(), vec![0xFF, 0xFE, 0x00, 0xC3, 0x28]);
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvCommand::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn kv_command_write_to_roundtrip_get_non_utf8_key() {
        let original = KvCommand::Get(vec![0xFF, 0xFE, 0x00, 0xC3, 0x28]);
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvCommand::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn kv_command_write_to_roundtrip_del_non_utf8_key() {
        let original = KvCommand::Del(vec![0xFF, 0xFE, 0x00, 0xC3, 0x28]);
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvCommand::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }
}
