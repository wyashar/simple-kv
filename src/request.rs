use std::fmt;

use ParseError::{
    ArrayTooLong, ComplexStringTooLong, ExpectedArray, ExpectedComplexString, MissingCrlf,
    MissingFirstByte,
};
use thiserror::Error;

const ARRAY_BYTE: u8 = b'*';
const COMPLEX_STRING_BYTE: u8 = b'$';

const CRLF: &[u8; 2] = b"\r\n";

const MAX_COMPLEX_STRING_LENGTH: usize = 512 * 1024 * 1024; // 512 MB
const MAX_ARRAY_LENGTH: usize = 1024 * 1024;

pub struct Request {
    complex_strings: Vec<Vec<u8>>,
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, s) in self.complex_strings.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{}", String::from_utf8_lossy(s))?;
        }
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("request was empty; no first byte present")]
    MissingFirstByte,
    #[error("expected top-level array (*), got byte {0:?}")]
    ExpectedArray(u8),
    #[error("expected crlf")]
    MissingCrlf,
    #[error("invalid utf-8 in payload")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    #[error("invalid integer in payload")]
    InvalidInt(#[from] std::num::ParseIntError),
    #[error("array length {0} exceeds maximum {MAX_ARRAY_LENGTH}")]
    ArrayTooLong(usize),
    #[error("expected bulk string ($), got byte {0:?}")]
    ExpectedComplexString(u8),
    #[error("bulk string length {0} exceeds maximum {MAX_COMPLEX_STRING_LENGTH}")]
    ComplexStringTooLong(usize),
}

struct Parsed<T> {
    data: T,
    num_bytes: usize,
}

impl Request {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buffer: Vec<u8> = Vec::new();

        buffer.push(ARRAY_BYTE);
        buffer.extend_from_slice(self.complex_strings.len().to_string().as_bytes());
        buffer.extend_from_slice(CRLF);

        for s in &self.complex_strings {
            buffer.push(COMPLEX_STRING_BYTE);
            buffer.extend_from_slice(s.len().to_string().as_bytes());
            buffer.extend_from_slice(CRLF);
            buffer.extend_from_slice(s);
            buffer.extend_from_slice(CRLF);
        }

        buffer
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Request, ParseError> {
        let Some(first_byte) = bytes.first() else {
            return Err(MissingFirstByte);
        };

        if *first_byte != ARRAY_BYTE {
            return Err(ExpectedArray(*first_byte));
        }

        let header = Self::parse_until_crlf(&bytes[1..])?;
        let arr_len: usize = str::from_utf8(header.data)?.parse()?;
        let mut cursor = header.num_bytes + 1;

        if arr_len > MAX_ARRAY_LENGTH {
            return Err(ArrayTooLong(arr_len));
        }

        let mut complex_strings: Vec<Vec<u8>> = Vec::with_capacity(arr_len);

        for _ in 0..arr_len {
            let header = Self::parse_complex_string(&bytes[cursor..])?;
            complex_strings.push(header.data.to_owned());
            cursor += header.num_bytes;
        }

        Ok(Request { complex_strings })
    }

    fn parse_complex_string(bytes: &[u8]) -> Result<Parsed<&[u8]>, ParseError> {
        let Some(first_byte) = bytes.first() else {
            return Err(MissingFirstByte);
        };

        if *first_byte != COMPLEX_STRING_BYTE {
            return Err(ExpectedComplexString(*first_byte));
        }

        let mut cursor = 1;

        let header = Self::parse_until_crlf(&bytes[cursor..])?;
        let str_len: usize = str::from_utf8(header.data)?.parse()?;
        cursor += header.num_bytes;

        if str_len > MAX_COMPLEX_STRING_LENGTH {
            return Err(ComplexStringTooLong(str_len));
        }

        let data = &bytes[cursor..cursor + str_len];
        cursor += str_len;

        Self::check_for_crlf(&bytes[cursor..cursor + 2])?;
        cursor += 2;

        Ok(Parsed {
            data,
            num_bytes: cursor,
        })
    }

    fn parse_until_crlf(bytes: &[u8]) -> Result<Parsed<&[u8]>, ParseError> {
        let pos = bytes
            .windows(2)
            .position(|w| w == CRLF)
            .ok_or(MissingCrlf)?;

        Ok(Parsed {
            data: &bytes[..pos],
            num_bytes: pos + 2,
        })
    }

    fn check_for_crlf(bytes: &[u8]) -> Result<(), ParseError> {
        if bytes != CRLF {
            return Err(MissingCrlf);
        }

        Ok(())
    }
}
