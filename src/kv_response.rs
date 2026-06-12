use std::borrow::Cow;
use std::fmt;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;
use std::num::ParseIntError;
use std::str::Utf8Error;

#[derive(Debug, PartialEq)]
pub enum KvResponse<'a> {
    Okay,
    Error(String),
    Value(Cow<'a, [u8]>),
    NotFound,
}

const OKAY_PREFIX: u8 = b'+';
const ERROR_PREFIX: u8 = b'-';
const VALUE_PREFIX: u8 = b'$';
const NOTFOUND_PREFIX: u8 = b'!';
const PREFIXES: [char; 4] = [
    OKAY_PREFIX as char,
    ERROR_PREFIX as char,
    VALUE_PREFIX as char,
    NOTFOUND_PREFIX as char,
];
const CRLF: &[u8; 2] = b"\r\n";

#[derive(Debug, thiserror::Error)]
pub enum KvResponseError {
    #[error("unrecognized response prefix byte: {0:?}, expected one of {prefixes:?}", prefixes = PREFIXES)]
    BadFirstChar(char),
    #[error("response body was not valid UTF-8")]
    BadUtf8(#[from] Utf8Error),
    #[error("did not find a CRLF while parsing")]
    MissingCrlf,
    #[error("io error parsing the response")]
    Io(#[from] std::io::Error),
    #[error("keys and values should be valid usizes")]
    NonIntLength(#[from] ParseIntError),
}

impl<'a> KvResponse<'a> {
    fn prefix(&self) -> u8 {
        match self {
            Self::Okay => OKAY_PREFIX,
            Self::Error(_) => ERROR_PREFIX,
            Self::Value(_) => VALUE_PREFIX,
            Self::NotFound => NOTFOUND_PREFIX,
        }
    }

    pub fn from_reader<T: BufRead>(reader: &mut T) -> Result<KvResponse<'a>, KvResponseError> {
        let mut prefix_buf: [u8; 1] = [0; 1];
        reader.read_exact(&mut prefix_buf)?;

        match prefix_buf[0] {
            OKAY_PREFIX => {
                expect_crlf(reader)?;
                Ok(KvResponse::Okay)
            }
            ERROR_PREFIX => {
                let mut msg_bytes: Vec<u8> = Vec::new();
                reader.read_until(b'\n', &mut msg_bytes)?;
                let msg = msg_bytes
                    .strip_suffix(CRLF)
                    .ok_or(KvResponseError::MissingCrlf)?;
                Ok(KvResponse::Error(std::str::from_utf8(msg)?.to_owned()))
            }
            VALUE_PREFIX => {
                let mut val_len_bytes: Vec<u8> = Vec::new();
                reader.read_until(b'\n', &mut val_len_bytes)?;

                let val_len_bytes_trimmed = val_len_bytes
                    .strip_suffix(CRLF)
                    .ok_or(KvResponseError::MissingCrlf)?;

                let val_len: usize = std::str::from_utf8(val_len_bytes_trimmed)?.parse()?;
                let mut value: Vec<u8> = vec![0u8; val_len];
                reader.read_exact(&mut value)?;

                expect_crlf(reader)?;

                Ok(KvResponse::Value(Cow::Owned(value)))
            }
            NOTFOUND_PREFIX => {
                expect_crlf(reader)?;
                Ok(KvResponse::NotFound)
            }
            other => Err(KvResponseError::BadFirstChar(other as char)),
        }
    }

    pub fn write_to<W: Write>(&self, w: &mut W) -> Result<(), std::io::Error> {
        w.write_all(&[self.prefix()])?;

        match self {
            Self::Okay => {}
            Self::Error(e) => w.write_all(e.as_bytes())?,
            Self::Value(bytes) => {
                w.write_all(bytes.len().to_string().as_bytes())?;
                w.write_all(CRLF)?;
                w.write_all(&bytes)?
            }
            Self::NotFound => {}
        }

        w.write_all(CRLF)?;
        Ok(())
    }
}

fn expect_crlf<R: Read>(reader: &mut R) -> Result<(), KvResponseError> {
    let mut buf: [u8; 2] = [0; 2];
    reader.read_exact(&mut buf)?;
    if &buf != CRLF {
        return Err(KvResponseError::MissingCrlf);
    }
    Ok(())
}

impl fmt::Display for KvResponse<'_> {
    // Lossy: a Value's non-UTF8 bytes are replaced with U+FFFD. Display only, not for round-tripping.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Okay => write!(f, "Okay"),
            Self::Error(e) => write!(f, "Error: {e}"),
            Self::Value(bytes) => write!(f, "Value: {}", String::from_utf8_lossy(bytes)),
            Self::NotFound => write!(f, "NotFound"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_reader_okay() {
        let mut bytes: &[u8] = b"+\r\n";
        let actual = KvResponse::from_reader(&mut bytes);
        assert_eq!(actual.unwrap(), KvResponse::Okay);
    }

    #[test]
    fn from_reader_error() {
        let mut bytes: &[u8] = b"-something broke\r\n";
        let actual = KvResponse::from_reader(&mut bytes);
        assert_eq!(
            actual.unwrap(),
            KvResponse::Error("something broke".to_owned())
        );
    }

    #[test]
    fn from_reader_error_empty_message() {
        let mut bytes: &[u8] = b"-\r\n";
        let actual = KvResponse::from_reader(&mut bytes);
        assert_eq!(actual.unwrap(), KvResponse::Error(String::new()));
    }

    #[test]
    fn from_reader_unrecognized_first_char() {
        let mut bytes: &[u8] = b"*hello\r\n";
        let actual = KvResponse::from_reader(&mut bytes);
        assert!(matches!(actual, Err(KvResponseError::BadFirstChar('*'))));
    }

    #[test]
    fn from_reader_missing_crlf() {
        let mut bytes: &[u8] = b"+Okay";
        let actual = KvResponse::from_reader(&mut bytes);
        assert!(matches!(actual, Err(KvResponseError::MissingCrlf)));
    }

    #[test]
    fn from_reader_invalid_utf8_message() {
        let mut bytes: &[u8] = b"-\xff\xfe\r\n";
        let actual = KvResponse::from_reader(&mut bytes);
        assert!(matches!(actual, Err(KvResponseError::BadUtf8(_))));
    }

    #[test]
    fn write_to_roundtrip_okay() {
        let original = KvResponse::Okay;
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvResponse::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn write_to_roundtrip_error() {
        let original = KvResponse::Error("boom".to_owned());
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvResponse::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn write_to_roundtrip_error_empty() {
        let original = KvResponse::Error(String::new());
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvResponse::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn from_reader_value() {
        let mut bytes: &[u8] = b"$5\r\nhello\r\n";
        let actual = KvResponse::from_reader(&mut bytes);
        assert_eq!(
            actual.unwrap(),
            KvResponse::Value(Cow::Owned(b"hello".to_vec()))
        );
    }

    #[test]
    fn from_reader_not_found() {
        let mut bytes: &[u8] = b"!\r\n";
        let actual = KvResponse::from_reader(&mut bytes);
        assert_eq!(actual.unwrap(), KvResponse::NotFound);
    }

    #[test]
    fn write_to_roundtrip_value() {
        let original = KvResponse::Value(Cow::Owned(b"my-value".to_vec()));
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvResponse::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn write_to_roundtrip_value_non_utf8() {
        let original = KvResponse::Value(Cow::Owned(vec![0xFF, 0xFE, 0x00, 0xC3, 0x28]));
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvResponse::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn write_to_roundtrip_not_found() {
        let original = KvResponse::NotFound;
        let mut bytes: Vec<u8> = Vec::new();
        original.write_to(&mut bytes).expect("works");
        let roundtripped = KvResponse::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }
}
