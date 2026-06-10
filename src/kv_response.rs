use std::io::BufRead;

#[derive(Debug, PartialEq)]
pub enum KvResponse {
    Okay,
    Error(String),
}

#[derive(Debug, thiserror::Error)]
pub enum KvResponseError {
    #[error("response line was empty; expected a prefix byte")]
    EmptyLine,
    #[error("unrecognized response prefix byte: {0:?}")]
    BadFirstChar(char),
    #[error("response body was not valid UTF-8")]
    BadUTF8(#[from] std::str::Utf8Error),
    #[error("did not find a CRLF while parsing")]
    MissingCrlf,
    #[error("io error parsing the response")]
    Io(#[from] std::io::Error),
}

impl KvResponse {
    pub fn from_reader<T: BufRead>(reader: &mut T) -> Result<KvResponse, KvResponseError> {
        let mut line: Vec<u8> = Vec::new();
        reader.read_until(b'\n', &mut line)?;

        let payload = line
            .strip_suffix(b"\r\n")
            .ok_or(KvResponseError::MissingCrlf)?;

        let (prefix, rest) = payload.split_first().ok_or(KvResponseError::EmptyLine)?;

        match prefix {
            b'+' => Ok(KvResponse::Okay),
            b'-' => {
                let msg = std::str::from_utf8(rest)?;
                Ok(KvResponse::Error(msg.to_owned()))
            }
            other => Err(KvResponseError::BadFirstChar(*other as char)),
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.into()
    }
}

impl From<KvResponse> for Vec<u8> {
    fn from(kv_response: KvResponse) -> Vec<u8> {
        let mut buffer: Vec<u8> = Vec::new();
        match kv_response {
            KvResponse::Okay => buffer.extend_from_slice(b"+Okay\r\n"),
            KvResponse::Error(str) => {
                buffer.push(b'-');
                buffer.extend_from_slice(str.as_bytes());
                buffer.extend_from_slice(b"\r\n");
            }
        }

        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_reader_okay() {
        let mut bytes: &[u8] = b"+Okay\r\n";
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
    fn from_reader_empty_line() {
        let mut bytes: &[u8] = b"\r\n";
        let actual = KvResponse::from_reader(&mut bytes);
        assert!(matches!(actual, Err(KvResponseError::EmptyLine)));
    }

    #[test]
    fn from_reader_invalid_utf8_message() {
        let mut bytes: &[u8] = b"-\xff\xfe\r\n";
        let actual = KvResponse::from_reader(&mut bytes);
        assert!(matches!(actual, Err(KvResponseError::BadUTF8(_))));
    }

    #[test]
    fn into_bytes_roundtrip_okay() {
        let original = KvResponse::Okay;
        let bytes: Vec<u8> = KvResponse::Okay.into();
        let roundtripped = KvResponse::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn into_bytes_roundtrip_error() {
        let original = KvResponse::Error("boom".to_owned());
        let bytes: Vec<u8> = KvResponse::Error("boom".to_owned()).into();
        let roundtripped = KvResponse::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }

    #[test]
    fn into_bytes_roundtrip_error_empty() {
        let original = KvResponse::Error(String::new());
        let bytes: Vec<u8> = KvResponse::Error(String::new()).into();
        let roundtripped = KvResponse::from_reader(&mut &bytes[..]).expect("valid bytes");
        assert_eq!(original, roundtripped);
    }
}
