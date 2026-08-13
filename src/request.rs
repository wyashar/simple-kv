use log::debug;
use thiserror::Error;

use crate::request::ParseError::{
    ArrayTooLong, ComplexStringTooLong, ExpectedArray, ExpectedCString, MissingCrlf,
    MissingFirstByte,
};

const CRLF: &'static [u8; 2] = b"\r\n";
const MAX_COMPLEX_STRING_LENGTH: usize = 512 * 1024 * 1024; // 512 MB
const MAX_ARRAY_LENGTH: usize = 1024 * 1024;
const ARRAY_BYTE: u8 = b'*';
const CSTRING_BYTE: u8 = b'$';

pub struct Request {
    cstrs: Vec<Vec<u8>>,
}

pub struct RequestParser {
    g_idx: usize,
    arr_len: Option<usize>,
    q_buff: Vec<u8>,
    cs_buff: Vec<Vec<u8>>,
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
    #[error("expected complex string ($), got byte {0:?}")]
    ExpectedCString(u8),
    #[error("bulk string length {0} exceeds maximum {MAX_COMPLEX_STRING_LENGTH}")]
    ComplexStringTooLong(usize),
}

impl RequestParser {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Option<Request>, ParseError> {
        self.q_buff.extend_from_slice(bytes);

        if self.arr_len.is_none() {
            let Some(arr_header) = Self::parse_header(&self.q_buff, ARRAY_BYTE, ExpectedArray)?
            else {
                debug!("array header payload is missing CRLF, waiting");
                return Ok(None);
            };

            if arr_header.data > MAX_ARRAY_LENGTH {
                return Err(ArrayTooLong(arr_header.data));
            }

            self.arr_len = Some(arr_header.data);
            self.g_idx = arr_header.num_bytes_parsed;
        }

        while self.cs_buff.len() < self.arr_len.expect("arr_len set above") {
            let Some(cstr) = self.parse_cstr()? else {
                return Ok(None);
            };
            self.cs_buff.push(cstr);
        }

        Ok(Some(Request {
            cstrs: self.take_request(),
        }))
    }

    fn parse_cstr(&mut self) -> Result<Option<Vec<u8>>, ParseError> {
        if self.g_idx >= self.q_buff.len() {
            return Ok(None);
        }

        let Some(cstr_header) =
            Self::parse_header(&self.q_buff[self.g_idx..], CSTRING_BYTE, ExpectedCString)?
        else {
            debug!("cstr header payload is missing CRLF, waiting");
            return Ok(None);
        };

        let cstr_len = cstr_header.data;
        if cstr_len > MAX_COMPLEX_STRING_LENGTH {
            return Err(ComplexStringTooLong(cstr_len));
        }

        let data_start = self.g_idx + cstr_header.num_bytes_parsed;
        let cstr_crlf_idx = data_start + cstr_len + 2;

        if cstr_crlf_idx > self.q_buff.len() {
            debug!("cstr payload not fully buffered yet, waiting");
            return Ok(None);
        }

        Self::check_for_crlf(&self.q_buff[data_start + cstr_len..cstr_crlf_idx])?;
        let cstr = self.q_buff[data_start..data_start + cstr_len].to_owned();

        self.g_idx = cstr_crlf_idx;
        Ok(Some(cstr))
    }

    fn take_request(&mut self) -> Vec<Vec<u8>> {
        self.q_buff.drain(..self.g_idx);
        self.g_idx = 0;
        self.arr_len = None;

        std::mem::take(&mut self.cs_buff)
    }

    fn parse_header(
        bytes: &[u8],
        header_byte: u8,
        parse_error: fn(u8) -> ParseError,
    ) -> Result<Option<Parsed<usize>>, ParseError> {
        let Some(header) = Self::parse_line(bytes) else {
            debug!("missing CRLF in header payload");
            return Ok(None);
        };

        let Some(first) = header.data.first() else {
            return Err(MissingFirstByte);
        };

        if *first != header_byte {
            return Err(parse_error(*first));
        }

        let data: usize = str::from_utf8(&header.data[1..])?.parse()?;
        let num_bytes_parsed = header.num_bytes_parsed;

        Ok(Some(Parsed::new(data, num_bytes_parsed)))
    }

    fn check_for_crlf(bytes: &[u8]) -> Result<(), ParseError> {
        if bytes != CRLF {
            return Err(MissingCrlf);
        }

        Ok(())
    }

    fn parse_line(bytes: &[u8]) -> Option<Parsed<&[u8]>> {
        let crlf_pos = bytes.windows(2).position(|w| w == CRLF)?;
        let data = &bytes[..crlf_pos];
        let num_bytes_parsed = crlf_pos + 2;

        Some(Parsed::new(data, num_bytes_parsed))
    }
}

struct Parsed<T> {
    data: T,
    num_bytes_parsed: usize,
}

impl<T> Parsed<T> {
    pub fn new(data: T, num_bytes_parsed: usize) -> Parsed<T> {
        Parsed {
            data,
            num_bytes_parsed,
        }
    }
}
