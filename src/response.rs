use log::debug;
use thiserror::Error;

use crate::response::ParseError::{
    CStringTooLong, InvalidCStringLength, MissingCrlf, Poisoned, UnexpectedPrefix,
    UnexpectedSstr,
};
use crate::util::{CRLF, CSTRING_BYTE, MAX_COMPLEX_STRING_LENGTH, parse_line};

const SSTR_BYTE: u8 = b'+';
const ERROR_BYTE: u8 = b'-';
const INTEGER_BYTE: u8 = b':';
const NULL_CSTR_LEN: i64 = -1;
const OK_BODY: &[u8] = b"OK";

#[derive(Debug, PartialEq)]
pub enum Response {
    Ok,
    Error(String),
    Cstr(Vec<u8>),
    Null,
    Integer(i64),
}

#[derive(Default)]
pub struct ResponseParser {
    q_buff: Vec<u8>,
    poisoned: bool,
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("expected crlf")]
    MissingCrlf,
    #[error("invalid utf-8 in payload")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    #[error("invalid integer in payload")]
    InvalidInt(#[from] std::num::ParseIntError),
    #[error("unexpected response prefix byte {0:?}")]
    UnexpectedPrefix(u8),
    #[error("expected sstr OK, got {0:?}")]
    UnexpectedSstr(String),
    #[error("complex string length {0} exceeds maximum {MAX_COMPLEX_STRING_LENGTH}")]
    CStringTooLong(usize),
    #[error("invalid complex string length {0}")]
    InvalidCStringLength(i64),
    #[error("parser was reused after a previous error")]
    Poisoned,
}

impl ResponseParser {
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.q_buff.extend_from_slice(bytes);
    }

    pub fn parse_next(&mut self) -> Result<Option<Response>, ParseError> {
        if self.poisoned {
            return Err(Poisoned);
        }

        let result = self.parse_next_internal();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    fn parse_next_internal(&mut self) -> Result<Option<Response>, ParseError> {
        let Some(first) = self.q_buff.first().copied() else {
            return Ok(None);
        };

        match first {
            SSTR_BYTE => self.parse_sstr(),
            ERROR_BYTE => self.parse_error(),
            CSTRING_BYTE => self.parse_cstr(),
            INTEGER_BYTE => self.parse_integer(),
            other => Err(UnexpectedPrefix(other)),
        }
    }

    fn parse_sstr(&mut self) -> Result<Option<Response>, ParseError> {
        let Some(line) = parse_line(&self.q_buff) else {
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
        let Some(line) = parse_line(&self.q_buff) else {
            debug!("error string is missing CRLF, waiting");
            return Ok(None);
        };

        let msg = str::from_utf8(&line.data[1..])?.to_owned();
        self.take(line.num_bytes_parsed);
        Ok(Some(Response::Error(msg)))
    }

    fn parse_integer(&mut self) -> Result<Option<Response>, ParseError> {
        let Some(line) = parse_line(&self.q_buff) else {
            debug!("integer is missing CRLF, waiting");
            return Ok(None);
        };

        let n: i64 = str::from_utf8(&line.data[1..])?.parse()?;
        self.take(line.num_bytes_parsed);
        Ok(Some(Response::Integer(n)))
    }

    fn parse_cstr(&mut self) -> Result<Option<Response>, ParseError> {
        let Some(header) = parse_line(&self.q_buff) else {
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

        if cstr_crlf_idx > self.q_buff.len() {
            debug!("cstr payload not fully buffered yet, waiting");
            return Ok(None);
        }

        if &self.q_buff[data_start + len..cstr_crlf_idx] != CRLF {
            return Err(MissingCrlf);
        }

        let cstr = self.q_buff[data_start..data_start + len].to_owned();
        self.take(cstr_crlf_idx);
        Ok(Some(Response::Cstr(cstr)))
    }

    fn take(&mut self, num_bytes: usize) {
        self.q_buff.drain(..num_bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_parse(
        parser: &mut ResponseParser,
        bytes: &[u8],
    ) -> Result<Option<Response>, ParseError> {
        parser.push_bytes(bytes);
        parser.parse_next()
    }

    fn assert_parses(input: &[u8], expected: Response) {
        let mut parser = ResponseParser::default();
        let response = push_parse(&mut parser, input)
            .expect("parse should succeed")
            .expect("response should be complete");

        assert_eq!(response, expected);
        assert!(
            parser.parse_next().expect("no error").is_none(),
            "parser should have consumed the full input"
        );
    }

    fn parse_error(input: &[u8]) -> ParseError {
        let mut parser = ResponseParser::default();
        push_parse(&mut parser, input).expect_err("parse should fail")
    }

    fn assert_incomplete(input: &[u8]) {
        let mut parser = ResponseParser::default();
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
    fn empty_cstr_is_not_null() {
        let mut parser = ResponseParser::default();
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
        let mut parser = ResponseParser::default();

        assert!(push_parse(&mut parser, b"$5\r\n").expect("no error").is_none());
        assert!(push_parse(&mut parser, b"hel").expect("no error").is_none());

        let response = push_parse(&mut parser, b"lo\r\n")
            .expect("no error")
            .expect("response should now be complete");

        assert_eq!(response, Response::Cstr(b"hello".to_vec()));
    }

    #[test]
    fn parse_next_yields_second_response_without_more_bytes() {
        let mut parser = ResponseParser::default();
        parser.push_bytes(b"+OK\r\n:1\r\n");

        assert_eq!(
            parser.parse_next().unwrap(),
            Some(Response::Ok)
        );
        assert_eq!(
            parser.parse_next().unwrap(),
            Some(Response::Integer(1))
        );
        assert!(parser.parse_next().unwrap().is_none());
    }

    #[test]
    fn errors_on_unexpected_prefix() {
        let err = parse_error(b"*2\r\n");
        assert!(matches!(err, ParseError::UnexpectedPrefix(b'*')));
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
    fn parser_is_poisoned_after_an_error() {
        let mut parser = ResponseParser::default();

        let err = push_parse(&mut parser, b"*1\r\n").expect_err("bad prefix should error");
        assert!(matches!(err, ParseError::UnexpectedPrefix(b'*')));

        parser.push_bytes(b"+OK\r\n");
        let err = parser
            .parse_next()
            .expect_err("poisoned parser should reject further input");
        assert!(matches!(err, ParseError::Poisoned));
    }
}
