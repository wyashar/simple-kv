use std::fmt;
use std::io::BufRead;

use log::debug;
use thiserror::Error;

use crate::response::ParseError::{
    CStringTooLong, InvalidArrayLength, InvalidCStringLength, MissingCrlf, Poisoned, UnexpectedEof,
    UnexpectedPrefix, UnexpectedSstr,
};
use crate::util::{Bytes, CRLF, CSTRING_BYTE, MAX_COMPLEX_STRING_LENGTH, parse_line};

const SSTR_BYTE: u8 = b'+';
const ERROR_BYTE: u8 = b'-';
const INTEGER_BYTE: u8 = b':';
const ARRAY_BYTE: u8 = b'*';
const NULL_CSTR_LEN: i64 = -1;
const OK_BODY: &[u8] = b"OK";
const PREFIX_BYTES: [u8; 5] = [
    SSTR_BYTE,
    ERROR_BYTE,
    CSTRING_BYTE,
    INTEGER_BYTE,
    ARRAY_BYTE,
];

#[derive(Debug, PartialEq)]
pub enum Response<T = Bytes> {
    Ok,
    Error(String),
    Cstr(T),
    Null,
    Integer(i64),
    Array(Vec<Response<T>>),
}

impl<B: AsRef<[u8]>> Response<B> {
    pub fn prefix_byte(&self) -> u8 {
        match self {
            Self::Ok => SSTR_BYTE,
            Self::Error(_) => ERROR_BYTE,
            Self::Cstr(_) | Self::Null => CSTRING_BYTE,
            Self::Integer(_) => INTEGER_BYTE,
            Self::Array(_) => ARRAY_BYTE,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![self.prefix_byte()];
        match self {
            Self::Ok => buf.extend_from_slice(OK_BODY),
            Self::Error(msg) => buf.extend_from_slice(msg.as_bytes()),
            Self::Cstr(bytes) => {
                let bytes = bytes.as_ref();
                buf.extend_from_slice(bytes.len().to_string().as_bytes());
                buf.extend_from_slice(CRLF);
                buf.extend_from_slice(bytes);
            }
            Self::Null => buf.extend_from_slice(b"-1"),
            Self::Integer(n) => buf.extend_from_slice(n.to_string().as_bytes()),
            Self::Array(responses) => {
                buf.extend_from_slice(responses.len().to_string().as_bytes());
                buf.extend_from_slice(CRLF);
                for response in responses {
                    buf.extend_from_slice(&response.to_bytes());
                }
                return buf;
            }
        }
        buf.extend_from_slice(CRLF);
        buf
    }
}

impl<B: AsRef<[u8]>> fmt::Display for Response<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => f.write_str("OK"),
            Self::Error(msg) => write!(f, "{msg}"),
            Self::Cstr(bytes) => write!(f, "{}", String::from_utf8_lossy(bytes.as_ref())),
            Self::Null => f.write_str("(nil)"),
            Self::Integer(n) => write!(f, "{n}"),
            Self::Array(responses) => {
                for (index, response) in responses.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{response}")?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Default)]
struct ResponseDecoder {
    buffer: Vec<u8>,
    poisoned: bool,
}

pub struct ResponseReader<R> {
    reader: R,
    decoder: ResponseDecoder,
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("expected crlf")]
    MissingCrlf,
    #[error("invalid utf-8 in payload")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    #[error("invalid integer in payload")]
    InvalidInt(#[from] std::num::ParseIntError),
    #[error("unexpected response prefix byte, expected one of {prefixes:?}, but got: {got:?}", prefixes = PREFIX_BYTES, got = *.0 as char)]
    UnexpectedPrefix(u8),
    #[error("expected sstr OK, got {0:?}")]
    UnexpectedSstr(String),
    #[error("complex string length {0} exceeds maximum {MAX_COMPLEX_STRING_LENGTH}")]
    CStringTooLong(usize),
    #[error("invalid complex string length {0}")]
    InvalidCStringLength(i64),
    #[error("invalid array length {0}")]
    InvalidArrayLength(i64),
    #[error("failed to read response: {0}")]
    Io(#[from] std::io::Error),
    #[error("decoder was reused after a previous error")]
    Poisoned,
    #[error("unexpected EOF during response parsing")]
    UnexpectedEof,
}

impl<R> ResponseReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            decoder: ResponseDecoder::default(),
        }
    }

    pub fn get_reader_mut(&mut self) -> &mut R {
        &mut self.reader
    }

    pub fn into_inner(self) -> R {
        self.reader
    }
}

impl<R: BufRead> ResponseReader<R> {
    pub fn read_next(&mut self) -> Result<Option<Response>, ParseError> {
        loop {
            if let Some(response) = self.decoder.decode_next()? {
                return Ok(Some(response));
            }

            let num_bytes_read = {
                let bytes = self.reader.fill_buf()?;
                if bytes.is_empty() {
                    self.decoder.validate_eof()?;
                    return Ok(None);
                }

                self.decoder.buffer.extend_from_slice(bytes);
                bytes.len()
            };

            self.reader.consume(num_bytes_read);
        }
    }
}

impl ResponseDecoder {
    fn validate_eof(&self) -> Result<(), ParseError> {
        if self.poisoned {
            return Err(Poisoned);
        }

        if !self.buffer.is_empty() {
            return Err(UnexpectedEof);
        }

        Ok(())
    }

    fn decode_next(&mut self) -> Result<Option<Response>, ParseError> {
        if self.poisoned {
            return Err(Poisoned);
        }

        let result = self.decode_buffered();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    fn decode_buffered(&mut self) -> Result<Option<Response>, ParseError> {
        let Some(first) = self.buffer.first().copied() else {
            return Ok(None);
        };

        match first {
            SSTR_BYTE => self.parse_sstr(),
            ERROR_BYTE => self.parse_error(),
            CSTRING_BYTE => self.parse_cstr(),
            INTEGER_BYTE => self.parse_integer(),
            ARRAY_BYTE => self.parse_array(),
            other => Err(UnexpectedPrefix(other)),
        }
    }

    fn parse_sstr(&mut self) -> Result<Option<Response>, ParseError> {
        let Some(line) = parse_line(&self.buffer) else {
            debug!("sstr is missing CRLF, waiting");
            return Ok(None);
        };

        if &line.data[1..] != OK_BODY {
            return Err(UnexpectedSstr(
                String::from_utf8_lossy(&line.data[1..]).into_owned(),
            ));
        }

        self.take(line.num_bytes_parsed);
        Ok(Some(Response::Ok))
    }

    fn parse_error(&mut self) -> Result<Option<Response>, ParseError> {
        let Some(line) = parse_line(&self.buffer) else {
            debug!("error string is missing CRLF, waiting");
            return Ok(None);
        };

        let msg = str::from_utf8(&line.data[1..])?.to_owned();
        self.take(line.num_bytes_parsed);
        Ok(Some(Response::Error(msg)))
    }

    fn parse_integer(&mut self) -> Result<Option<Response>, ParseError> {
        let Some(line) = parse_line(&self.buffer) else {
            debug!("integer is missing CRLF, waiting");
            return Ok(None);
        };

        let n: i64 = str::from_utf8(&line.data[1..])?.parse()?;
        self.take(line.num_bytes_parsed);
        Ok(Some(Response::Integer(n)))
    }

    fn parse_cstr(&mut self) -> Result<Option<Response>, ParseError> {
        let Some(header) = parse_line(&self.buffer) else {
            debug!("cstr header payload is missing CRLF, waiting");
            return Ok(None);
        };

        let len: i64 = str::from_utf8(&header.data[1..])?.parse()?;
        if len == NULL_CSTR_LEN {
            self.take(header.num_bytes_parsed);
            return Ok(Some(Response::Null));
        }
        if len < 0 {
            return Err(InvalidCStringLength(len));
        }

        let len = len as usize;
        if len > MAX_COMPLEX_STRING_LENGTH {
            return Err(CStringTooLong(len));
        }

        let data_start = header.num_bytes_parsed;
        let cstr_crlf_idx = data_start + len + 2;

        if cstr_crlf_idx > self.buffer.len() {
            debug!("cstr payload not fully buffered yet, waiting");
            return Ok(None);
        }

        if &self.buffer[data_start + len..cstr_crlf_idx] != CRLF {
            return Err(MissingCrlf);
        }

        let cstr = self.buffer[data_start..data_start + len].to_owned();
        self.take(cstr_crlf_idx);
        Ok(Some(Response::Cstr(cstr)))
    }

    fn parse_array(&mut self) -> Result<Option<Response>, ParseError> {
        let Some(header) = parse_line(&self.buffer) else {
            debug!("array header is missing CRLF, waiting");
            return Ok(None);
        };

        let len: i64 = str::from_utf8(&header.data[1..])?.parse()?;
        if len < 0 {
            return Err(InvalidArrayLength(len));
        }

        let mut responses = Vec::with_capacity(len as usize);
        let mut num_bytes_parsed = header.num_bytes_parsed;
        for _ in 0..len {
            let remaining = self.buffer[num_bytes_parsed..].to_vec();
            let remaining_len = remaining.len();
            let mut decoder = ResponseDecoder {
                buffer: remaining,
                poisoned: false,
            };
            let Some(response) = decoder.decode_buffered()? else {
                debug!("array element is not fully buffered yet, waiting");
                return Ok(None);
            };
            num_bytes_parsed += remaining_len - decoder.buffer.len();
            responses.push(response);
        }

        self.take(num_bytes_parsed);
        Ok(Some(Response::Array(responses)))
    }

    fn take(&mut self, num_bytes: usize) {
        self.buffer.drain(..num_bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_parse(
        parser: &mut ResponseDecoder,
        bytes: &[u8],
    ) -> Result<Option<Response>, ParseError> {
        parser.buffer.extend_from_slice(bytes);
        parser.decode_next()
    }

    fn assert_parses(input: &[u8], expected: Response) {
        let mut parser = ResponseDecoder::default();
        let response = push_parse(&mut parser, input)
            .expect("parse should succeed")
            .expect("response should be complete");

        assert_eq!(response, expected);
        assert!(
            parser.decode_next().expect("no error").is_none(),
            "parser should have consumed the full input"
        );
    }

    fn parse_error(input: &[u8]) -> ParseError {
        let mut parser = ResponseDecoder::default();
        push_parse(&mut parser, input).expect_err("parse should fail")
    }

    fn assert_incomplete(input: &[u8]) {
        let mut parser = ResponseDecoder::default();
        let result = push_parse(&mut parser, input).expect("parse should not error");
        assert!(result.is_none(), "expected incomplete, got a full response");
    }

    #[test]
    fn parses_ok() {
        assert_parses(b"+OK\r\n", Response::Ok);
    }

    #[test]
    fn parses_error() {
        assert_parses(
            b"-ERR unknown command 'MYKEY'\r\n",
            Response::Error("ERR unknown command 'MYKEY'".to_owned()),
        );
    }

    #[test]
    fn parses_empty_error() {
        assert_parses(b"-\r\n", Response::Error(String::new()));
    }

    #[test]
    fn parses_cstr() {
        assert_parses(b"$5\r\nhello\r\n", Response::Cstr(b"hello".to_vec()));
    }

    #[test]
    fn parses_empty_cstr() {
        assert_parses(b"$0\r\n\r\n", Response::Cstr(vec![]));
    }

    #[test]
    fn parses_null_cstr() {
        assert_parses(b"$-1\r\n", Response::Null);
    }

    #[test]
    fn parses_integer() {
        assert_parses(b":2\r\n", Response::Integer(2));
    }

    #[test]
    fn parses_negative_integer() {
        assert_parses(b":-1\r\n", Response::Integer(-1));
    }

    #[test]
    fn parses_array() {
        assert_parses(
            b"*3\r\n$5\r\nvalue\r\n$-1\r\n:2\r\n",
            Response::Array(vec![
                Response::Cstr(b"value".to_vec()),
                Response::Null,
                Response::Integer(2),
            ]),
        );
    }

    #[test]
    fn incomplete_array_does_not_consume_buffered_elements() {
        let mut parser = ResponseDecoder::default();
        assert!(
            push_parse(&mut parser, b"*2\r\n$3\r\none\r\n")
                .expect("no error")
                .is_none()
        );

        assert_eq!(
            push_parse(&mut parser, b"$3\r\ntwo\r\n").expect("no error"),
            Some(Response::Array(vec![
                Response::Cstr(b"one".to_vec()),
                Response::Cstr(b"two".to_vec()),
            ]))
        );
    }

    #[test]
    fn empty_cstr_is_not_null() {
        let mut parser = ResponseDecoder::default();
        assert_eq!(
            push_parse(&mut parser, b"$0\r\n\r\n").unwrap(),
            Some(Response::Cstr(vec![]))
        );
        assert_eq!(
            push_parse(&mut parser, b"$-1\r\n").unwrap(),
            Some(Response::Null)
        );
    }

    #[test]
    fn ok_without_crlf_is_incomplete() {
        assert_incomplete(b"+OK");
    }

    #[test]
    fn cstr_header_without_crlf_is_incomplete() {
        assert_incomplete(b"$5");
    }

    #[test]
    fn cstr_payload_incomplete() {
        assert_incomplete(b"$5\r\nhel");
    }

    #[test]
    fn declared_null_does_not_wait_for_body() {
        assert_parses(b"$-1\r\n", Response::Null);
    }

    #[test]
    fn completes_cstr_fed_in_chunks() {
        let mut parser = ResponseDecoder::default();

        assert!(
            push_parse(&mut parser, b"$5\r\n")
                .expect("no error")
                .is_none()
        );
        assert!(push_parse(&mut parser, b"hel").expect("no error").is_none());

        let response = push_parse(&mut parser, b"lo\r\n")
            .expect("no error")
            .expect("response should now be complete");

        assert_eq!(response, Response::Cstr(b"hello".to_vec()));
    }

    #[test]
    fn decode_next_yields_second_response_without_more_bytes() {
        let mut parser = ResponseDecoder::default();
        parser.buffer.extend_from_slice(b"+OK\r\n:1\r\n");

        assert_eq!(parser.decode_next().unwrap(), Some(Response::Ok));
        assert_eq!(parser.decode_next().unwrap(), Some(Response::Integer(1)));
        assert!(parser.decode_next().unwrap().is_none());
    }

    #[test]
    fn errors_on_unexpected_prefix() {
        let err = parse_error(b"?2\r\n");
        assert!(matches!(err, ParseError::UnexpectedPrefix(b'?')));
    }

    #[test]
    fn errors_on_sstr_that_is_not_ok() {
        let err = parse_error(b"+PONG\r\n");
        assert!(matches!(
            err,
            ParseError::UnexpectedSstr(s) if s == "PONG"
        ));
    }

    #[test]
    fn errors_on_missing_crlf_after_cstr_payload() {
        let err = parse_error(b"$3\r\nGETXX");
        assert!(matches!(err, ParseError::MissingCrlf));
    }

    #[test]
    fn errors_on_invalid_cstr_length() {
        let err = parse_error(b"$-2\r\n");
        assert!(matches!(err, ParseError::InvalidCStringLength(-2)));
    }

    #[test]
    fn errors_on_non_utf8_error_message() {
        let err = parse_error(b"-\xff\xfe\r\n");
        assert!(matches!(err, ParseError::InvalidUtf8(_)));
    }

    #[test]
    fn decoder_is_poisoned_after_an_error() {
        let mut parser = ResponseDecoder::default();

        let err = push_parse(&mut parser, b"?1\r\n").expect_err("bad prefix should error");
        assert!(matches!(err, ParseError::UnexpectedPrefix(b'?')));

        parser.buffer.extend_from_slice(b"+OK\r\n");
        let err = parser
            .decode_next()
            .expect_err("poisoned decoder should reject further input");
        assert!(matches!(err, ParseError::Poisoned));
    }

    #[test]
    fn reader_reads_buffered_responses_one_at_a_time() {
        let mut reader = ResponseReader::new(std::io::Cursor::new(b"+OK\r\n:1\r\n"));

        assert_eq!(reader.read_next().unwrap(), Some(Response::Ok));
        assert_eq!(reader.read_next().unwrap(), Some(Response::Integer(1)));
        assert_eq!(reader.read_next().unwrap(), None);
    }

    #[test]
    fn reader_errors_on_incomplete_response_at_eof() {
        let mut reader = ResponseReader::new(std::io::Cursor::new(b"$5\r\nhel"));

        assert!(matches!(reader.read_next(), Err(ParseError::UnexpectedEof)));
    }

    fn assert_round_trips(response: Response) {
        let mut parser = ResponseDecoder::default();
        parser.buffer.extend_from_slice(&response.to_bytes());
        let parsed = parser
            .decode_next()
            .expect("serialized response should be valid RESP")
            .expect("serialized response should be complete");
        assert_eq!(parsed, response);
    }

    #[test]
    fn round_trips_ok() {
        assert_round_trips(Response::Ok);
    }

    #[test]
    fn round_trips_error() {
        assert_round_trips(Response::Error("ERR unknown command".to_owned()));
    }

    #[test]
    fn round_trips_cstr() {
        assert_round_trips(Response::Cstr(b"hello".to_vec()));
    }

    #[test]
    fn round_trips_empty_cstr() {
        assert_round_trips(Response::Cstr(vec![]));
    }

    #[test]
    fn round_trips_null() {
        assert_round_trips(Response::Null);
    }

    #[test]
    fn round_trips_integer() {
        assert_round_trips(Response::Integer(2));
    }
}
