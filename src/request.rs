use std::fmt;

use log::debug;
use thiserror::Error;

use crate::request::ParseError::{
    ArrayTooLong, CStringTooLong, ExpectedArray, ExpectedCString, MissingCrlf, MissingFirstByte,
    Poisoned,
};
use crate::util::{CRLF, Parsed, parse_line};

const MAX_COMPLEX_STRING_LENGTH: usize = 512 * 1024 * 1024; // 512 MB
const MAX_ARRAY_LENGTH: usize = 1024 * 1024;
const ARRAY_BYTE: u8 = b'*';
const CSTRING_BYTE: u8 = b'$';

#[derive(Debug)]
pub struct Request {
    cstrs: Vec<Vec<u8>>,
}

#[derive(Default)]
pub struct RequestParser {
    g_idx: usize,
    arr_len: Option<usize>,
    q_buff: Vec<u8>,
    cs_buff: Vec<Vec<u8>>,
    poisoned: bool,
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
    CStringTooLong(usize),
    #[error("parser was reused after a previous error")]
    Poisoned,
}

impl RequestParser {
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.q_buff.extend_from_slice(bytes);
    }

    pub fn parse_next(&mut self) -> Result<Option<Request>, ParseError> {
        if self.poisoned {
            return Err(Poisoned);
        }

        let result = self.parse_next_internal();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    fn parse_next_internal(&mut self) -> Result<Option<Request>, ParseError> {
        if self.arr_len.is_none() {
            let Some(arr_header) = Self::parse_header(
                &self.q_buff,
                ARRAY_BYTE,
                MAX_ARRAY_LENGTH,
                ExpectedArray,
                ArrayTooLong,
            )?
            else {
                debug!("array header payload is missing CRLF, waiting");
                return Ok(None);
            };

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

        let Some(cstr_header) = Self::parse_header(
            &self.q_buff[self.g_idx..],
            CSTRING_BYTE,
            MAX_COMPLEX_STRING_LENGTH,
            ExpectedCString,
            CStringTooLong,
        )?
        else {
            debug!("cstr header payload is missing CRLF, waiting");
            return Ok(None);
        };

        let cstr_len = cstr_header.data;
        let data_start = self.g_idx + cstr_header.num_bytes_parsed;
        let cstr_crlf_idx = data_start + cstr_len + 2;

        if cstr_crlf_idx > self.q_buff.len() {
            debug!("cstr payload not fully buffered yet, waiting");
            return Ok(None);
        }

        if &self.q_buff[data_start + cstr_len..cstr_crlf_idx] != CRLF {
            return Err(MissingCrlf);
        }

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
        max_header_len: usize,
        expected_error: fn(u8) -> ParseError,
        too_long_error: fn(usize) -> ParseError,
    ) -> Result<Option<Parsed<usize>>, ParseError> {
        let Some(header) = parse_line(bytes) else {
            debug!("missing CRLF in header payload");
            return Ok(None);
        };

        let Some(first) = header.data.first() else {
            return Err(MissingFirstByte);
        };

        if *first != header_byte {
            return Err(expected_error(*first));
        }

        let header_len: usize = str::from_utf8(&header.data[1..])?.parse()?;
        if header_len > max_header_len {
            return Err(too_long_error(header_len));
        }

        Ok(Some(Parsed::new(header_len, header.num_bytes_parsed)))
    }
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, cstr) in self.cstrs.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{}", String::from_utf8_lossy(cstr))?;
        }
        Ok(())
    }
}

impl Request {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();

        buf.push(ARRAY_BYTE);
        buf.extend_from_slice(self.cstrs.len().to_string().as_bytes());
        buf.extend_from_slice(CRLF);

        for cstr in &self.cstrs {
            buf.push(CSTRING_BYTE);
            buf.extend_from_slice(cstr.len().to_string().as_bytes());
            buf.extend_from_slice(CRLF);
            buf.extend_from_slice(cstr);
            buf.extend_from_slice(CRLF);
        }

        buf
    }

    pub fn into_args(self) -> Vec<Vec<u8>> {
        self.cstrs
    }
}

#[cfg(test)]
impl Request {
    pub fn from_args(cstrs: Vec<Vec<u8>>) -> Request {
        Request { cstrs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_parse(
        parser: &mut RequestParser,
        bytes: &[u8],
    ) -> Result<Option<Request>, ParseError> {
        parser.push_bytes(bytes);
        parser.parse_next()
    }

    /// Assert that parsing `input` yields a complete request whose bulk strings
    /// match `expected`.
    fn assert_parses(input: &[u8], expected: Vec<Vec<u8>>) {
        let mut parser = RequestParser::default();
        let request = push_parse(&mut parser, input)
            .expect("parse should succeed")
            .expect("request should be complete");

        assert_eq!(request.cstrs, expected);
    }

    #[test]
    fn parses_del_one_key() {
        assert_parses(
            b"*2\r\n$3\r\nDEL\r\n$5\r\nMyKey\r\n",
            vec![b"DEL".to_vec(), b"MyKey".to_vec()],
        );
    }

    #[test]
    fn parses_get_one_key() {
        assert_parses(
            b"*2\r\n$3\r\nGET\r\n$5\r\nMYKEY\r\n",
            vec![b"GET".to_vec(), b"MYKEY".to_vec()],
        );
    }

    #[test]
    fn parses_set_key_value() {
        assert_parses(
            b"*3\r\n$3\r\nSET\r\n$5\r\nMyKey\r\n$8\r\nMYValue2\r\n",
            vec![b"SET".to_vec(), b"MyKey".to_vec(), b"MYValue2".to_vec()],
        );
    }

    /// Parse `input` into a request and assert re-serializing it yields the same
    /// bytes we started with.
    fn assert_round_trips(input: &[u8]) {
        let mut parser = RequestParser::default();
        let request = push_parse(&mut parser, input)
            .expect("parse should succeed")
            .expect("request should be complete");

        assert_eq!(request.to_bytes(), input);
    }

    #[test]
    fn round_trips_get() {
        assert_round_trips(b"*2\r\n$3\r\nGET\r\n$5\r\nMYKEY\r\n");
    }

    #[test]
    fn round_trips_set() {
        assert_round_trips(b"*3\r\n$3\r\nSET\r\n$5\r\nMyKey\r\n$8\r\nMYValue2\r\n");
    }

    #[test]
    fn round_trips_del() {
        assert_round_trips(b"*2\r\n$3\r\nDEL\r\n$5\r\nMyKey\r\n");
    }

    /// Parse `input` and return the parse error it produces.
    fn parse_error(input: &[u8]) -> ParseError {
        let mut parser = RequestParser::default();
        push_parse(&mut parser, input).expect_err("parse should fail")
    }

    #[test]
    fn errors_when_array_byte_is_wrong() {
        let err = parse_error(b"&2\r\n$3\r\nGET\r\n$5\r\nMYKEY\r\n");
        assert!(matches!(err, ParseError::ExpectedArray(b'&')));
    }

    #[test]
    fn errors_when_array_byte_is_missing() {
        let err = parse_error(b"\r\n$3\r\nGET\r\n$5\r\nMYKEY\r\n");
        assert!(matches!(err, ParseError::MissingFirstByte));
    }

    #[test]
    fn errors_when_cstr_byte_is_wrong() {
        let err = parse_error(b"*2\r\n&3\r\nGET\r\n$5\r\nMYKEY\r\n");
        assert!(matches!(err, ParseError::ExpectedCString(b'&')));
    }

    #[test]
    fn errors_when_cstr_byte_is_missing() {
        let err = parse_error(b"*2\r\n\r\nGET\r\n$5\r\nMYKEY\r\n");
        assert!(matches!(err, ParseError::MissingFirstByte));
    }

    /// Push `input` and assert the parser reports "need more bytes" (incomplete).
    fn assert_incomplete(input: &[u8]) {
        let mut parser = RequestParser::default();
        let result = push_parse(&mut parser, input).expect("parse should not error");
        assert!(result.is_none(), "expected incomplete, got a full request");
    }

    #[test]
    fn array_header_without_crlf_is_incomplete() {
        assert_incomplete(b"*2");
    }

    #[test]
    fn cstr_header_without_crlf_is_incomplete() {
        assert_incomplete(b"*2\r\n$3");
    }

    #[test]
    fn errors_on_missing_crlf_after_cstr_payload() {
        // "GET" is 3 bytes, but it's followed by "XX" instead of "\r\n".
        let err = parse_error(b"*1\r\n$3\r\nGETXX");
        assert!(matches!(err, ParseError::MissingCrlf));
    }

    #[test]
    fn errors_on_partial_crlf_after_cstr_payload() {
        // "GET" is 3 bytes, followed by "\rX": has the \r but not the \n.
        let err = parse_error(b"*1\r\n$3\r\nGET\rX");
        assert!(matches!(err, ParseError::MissingCrlf));
    }

    #[test]
    fn array_header_incomplete_until_crlf_arrives() {
        let mut parser = RequestParser::default();

        // No CRLF at all yet.
        assert!(push_parse(&mut parser, b"*2").expect("no error").is_none());
        // Ends in a lone '\r' — still not a full CRLF.
        assert!(push_parse(&mut parser, b"\r").expect("no error").is_none());
        // CRLF now complete, but there are no bulk strings yet.
        assert!(push_parse(&mut parser, b"\n").expect("no error").is_none());
    }

    #[test]
    fn completes_request_fed_in_chunks() {
        let mut parser = RequestParser::default();

        assert!(push_parse(&mut parser, b"*2\r\n").expect("no error").is_none());
        assert!(
            push_parse(&mut parser, b"$3\r\nGET")
                .expect("no error")
                .is_none()
        );
        assert!(
            push_parse(&mut parser, b"\r\n$5\r\nMYKE")
                .expect("no error")
                .is_none()
        );

        let request = push_parse(&mut parser, b"Y\r\n")
            .expect("no error")
            .expect("request should now be complete");

        assert_eq!(request.cstrs, vec![b"GET".to_vec(), b"MYKEY".to_vec()]);
    }

    #[test]
    fn declared_length_too_short_errors() {
        // Header says 3, but the body is "HELLO" (5). We read "HEL", then the
        // trailing check lands on "LO" instead of "\r\n".
        let err = parse_error(b"*1\r\n$3\r\nHELLO\r\n");
        assert!(matches!(err, ParseError::MissingCrlf));
    }

    #[test]
    fn declared_length_too_long_within_buffer_errors() {
        // Header says 5, but the body is "HI" (2). We read 5 bytes ("HI\r\nZ",
        // swallowing the real CRLF), then the trailing check lands on "ZZ".
        let err = parse_error(b"*1\r\n$5\r\nHI\r\nZZZ");
        assert!(matches!(err, ParseError::MissingCrlf));
    }

    #[test]
    fn declared_length_too_long_past_buffer_is_incomplete() {
        // Header says 5, but only "HI\r\n" follows, so the declared payload runs
        // past the buffer end — treated as incomplete, waiting for more bytes.
        assert_incomplete(b"*1\r\n$5\r\nHI\r\n");
    }

    #[test]
    fn partial_array_header_leaves_state_uncommitted() {
        let mut parser = RequestParser::default();

        // "*5\r" — no full CRLF yet, so the array header can't be committed.
        let result = push_parse(&mut parser, b"*5\r").expect("no error");

        assert!(result.is_none());
        assert_eq!(parser.arr_len, None);
        assert_eq!(parser.g_idx, 0);
        assert_eq!(parser.q_buff, b"*5\r".to_vec());
        assert!(parser.cs_buff.is_empty());
    }

    #[test]
    fn parser_is_poisoned_after_an_error() {
        let mut parser = RequestParser::default();

        let err = push_parse(&mut parser, b"&2\r\n$3\r\nGET\r\n")
            .expect_err("bad array byte should error");
        assert!(matches!(err, ParseError::ExpectedArray(b'&')));

        // Even a perfectly valid request is rejected after the parser is poisoned.
        parser.push_bytes(b"*1\r\n$4\r\nPING\r\n");
        let err = parser
            .parse_next()
            .expect_err("poisoned parser should reject further input");
        assert!(matches!(err, ParseError::Poisoned));
    }

    #[test]
    fn full_array_header_commits_state() {
        let mut parser = RequestParser::default();

        // "*5\r\n" — a complete array header, but no bulk strings yet.
        let result = push_parse(&mut parser, b"*5\r\n").expect("no error");

        assert!(result.is_none());
        assert_eq!(parser.arr_len, Some(5));
        assert_eq!(parser.g_idx, 4); // crlf pos (2) + 2, i.e. past "*5\r\n"
        assert_eq!(parser.q_buff, b"*5\r\n".to_vec());
        assert!(parser.cs_buff.is_empty());
    }

    #[test]
    fn good_array_then_bad_cstr_header_state() {
        let mut parser = RequestParser::default();

        // Array header is fine, but the first bulk string uses '&' not '$'.
        let err = push_parse(&mut parser, b"*2\r\n&3\r\nGET\r\n")
            .expect_err("bad cstr byte should error");

        assert!(matches!(err, ParseError::ExpectedCString(b'&')));
        assert!(parser.poisoned);
        assert_eq!(parser.arr_len, Some(2)); // array header committed
        assert_eq!(parser.g_idx, 4); // advanced past "*2\r\n" only
        assert!(parser.cs_buff.is_empty()); // no bulk string committed
    }

    #[test]
    fn good_array_good_cstr_header_bad_body_state() {
        let mut parser = RequestParser::default();

        // "$3" then "GET" is fine, but the payload is terminated by "XX".
        let err = push_parse(&mut parser, b"*1\r\n$3\r\nGETXX")
            .expect_err("bad cstr terminator should error");

        assert!(matches!(err, ParseError::MissingCrlf));
        assert!(parser.poisoned);
        assert_eq!(parser.arr_len, Some(1));
        assert_eq!(parser.g_idx, 4); // cstr not committed, so g_idx stays at the header
        assert!(parser.cs_buff.is_empty());
    }

    #[test]
    fn good_array_one_cstr_then_bad_next_cstr_state() {
        let mut parser = RequestParser::default();

        // First bulk string "GET" parses cleanly; the second uses '&' not '$'.
        let err = push_parse(&mut parser, b"*2\r\n$3\r\nGET\r\n&5\r\nMYKEY\r\n")
            .expect_err("bad second cstr byte should error");

        assert!(matches!(err, ParseError::ExpectedCString(b'&')));
        assert!(parser.poisoned);
        assert_eq!(parser.arr_len, Some(2));
        assert_eq!(parser.g_idx, 13); // advanced past "*2\r\n$3\r\nGET\r\n"
        assert_eq!(parser.cs_buff, vec![b"GET".to_vec()]); // first string committed
    }

    #[test]
    fn parse_next_yields_second_request_without_more_bytes() {
        let mut parser = RequestParser::default();
        parser.push_bytes(
            b"*2\r\n$3\r\nGET\r\n$1\r\na\r\n*2\r\n$3\r\nGET\r\n$1\r\nb\r\n",
        );

        let first = parser
            .parse_next()
            .expect("no error")
            .expect("first request should be complete");
        assert_eq!(first.cstrs, vec![b"GET".to_vec(), b"a".to_vec()]);

        let second = parser
            .parse_next()
            .expect("no error")
            .expect("second request should already be buffered");
        assert_eq!(second.cstrs, vec![b"GET".to_vec(), b"b".to_vec()]);

        assert!(
            parser.parse_next().expect("no error").is_none(),
            "no third request"
        );
    }
}
