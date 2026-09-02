use std::fmt;
use std::io::Read;

use log::debug;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::request::RequestParseError::{
    ArrayTooLong, CStringTooLong, ExpectedArray, ExpectedCString, MissingCrlf, MissingFirstByte,
    Poisoned, UnexpectedEof,
};
use crate::util::{Bytes, CRLF, CSTRING_BYTE, MAX_COMPLEX_STRING_LENGTH, Parsed, parse_line};

const MAX_ARRAY_LENGTH: usize = 1024 * 1024;
const ARRAY_BYTE: u8 = b'*';
const READ_CHUNK_SIZE: usize = 8192;

#[derive(Debug)]
pub struct Request {
    cstrs: Vec<Bytes>,
}

#[derive(Default)]
struct RequestDecoder {
    cursor: usize,
    expected_arg_count: Option<usize>,
    buffer: Vec<u8>,
    args: Vec<Bytes>,
    poisoned: bool,
}

pub struct RequestReader<R> {
    reader: R,
    decoder: RequestDecoder,
}

#[derive(Error, Debug)]
pub enum RequestParseError {
    #[error("request was empty; no first byte present")]
    MissingFirstByte,
    #[error("expected top-level array (*), got {ch:?}", ch = *.0 as char)]
    ExpectedArray(u8),
    #[error("expected crlf")]
    MissingCrlf,
    #[error("invalid utf-8 in payload")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    #[error("invalid integer in payload")]
    InvalidInt(#[from] std::num::ParseIntError),
    #[error("failed to read request: {0}")]
    Io(#[from] std::io::Error),
    #[error("array length {0} exceeds maximum {MAX_ARRAY_LENGTH}")]
    ArrayTooLong(usize),
    #[error("expected complex string ($), got {ch:?}", ch = *.0 as char)]
    ExpectedCString(u8),
    #[error("bulk string length {0} exceeds maximum {MAX_COMPLEX_STRING_LENGTH}")]
    CStringTooLong(usize),
    #[error("decoder was reused after a previous error")]
    Poisoned,
    #[error("unexpected EOF during request parsing")]
    UnexpectedEof,
}

impl<R> RequestReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            decoder: RequestDecoder::default(),
        }
    }

    pub fn into_inner(self) -> R {
        self.reader
    }
}

impl<R: AsyncRead + Unpin> RequestReader<R> {
    pub async fn read_next_async(&mut self) -> Result<Option<Request>, RequestParseError> {
        loop {
            if let Some(request) = self.decoder.decode_next()? {
                return Ok(Some(request));
            }

            let mut chunk = [0u8; READ_CHUNK_SIZE];
            let num_bytes_read = self.reader.read(&mut chunk).await?;
            if num_bytes_read == 0 {
                self.decoder.validate_eof()?;
                return Ok(None);
            }

            self.decoder
                .buffer
                .extend_from_slice(&chunk[..num_bytes_read]);
        }
    }
}

impl<R: Read> RequestReader<R> {
    pub fn read_next(&mut self) -> Result<Option<Request>, RequestParseError> {
        loop {
            if let Some(request) = self.decoder.decode_next()? {
                return Ok(Some(request));
            }

            let mut chunk = [0u8; READ_CHUNK_SIZE];
            let num_bytes_read = self.reader.read(&mut chunk)?;
            if num_bytes_read == 0 {
                self.decoder.validate_eof()?;
                return Ok(None);
            }

            self.decoder
                .buffer
                .extend_from_slice(&chunk[..num_bytes_read]);
        }
    }
}

impl RequestDecoder {
    fn validate_eof(&self) -> Result<(), RequestParseError> {
        if self.poisoned {
            return Err(Poisoned);
        }

        if !self.buffer.is_empty() {
            return Err(UnexpectedEof);
        }

        Ok(())
    }

    fn decode_next(&mut self) -> Result<Option<Request>, RequestParseError> {
        if self.poisoned {
            return Err(Poisoned);
        }

        let result = self.decode_buffered();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    fn decode_buffered(&mut self) -> Result<Option<Request>, RequestParseError> {
        if self.expected_arg_count.is_none() {
            let Some(arr_header) = Self::parse_header(
                &self.buffer,
                ARRAY_BYTE,
                MAX_ARRAY_LENGTH,
                ExpectedArray,
                ArrayTooLong,
            )?
            else {
                debug!("array header payload is missing CRLF, waiting");
                return Ok(None);
            };

            self.expected_arg_count = Some(arr_header.data);
            self.cursor = arr_header.num_bytes_parsed;
        }

        while self.args.len()
            < self
                .expected_arg_count
                .expect("expected_arg_count set above")
        {
            let Some(cstr) = self.parse_cstr()? else {
                return Ok(None);
            };
            self.args.push(cstr);
        }

        Ok(Some(Request {
            cstrs: self.take_request(),
        }))
    }

    fn parse_cstr(&mut self) -> Result<Option<Bytes>, RequestParseError> {
        if self.cursor >= self.buffer.len() {
            return Ok(None);
        }

        let Some(cstr_header) = Self::parse_header(
            &self.buffer[self.cursor..],
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
        let data_start = self.cursor + cstr_header.num_bytes_parsed;
        let cstr_crlf_idx = data_start + cstr_len + 2;

        if cstr_crlf_idx > self.buffer.len() {
            debug!("cstr payload not fully buffered yet, waiting");
            return Ok(None);
        }

        if &self.buffer[data_start + cstr_len..cstr_crlf_idx] != CRLF {
            return Err(MissingCrlf);
        }

        let cstr = self.buffer[data_start..data_start + cstr_len].to_owned();

        self.cursor = cstr_crlf_idx;
        Ok(Some(cstr))
    }

    fn take_request(&mut self) -> Vec<Bytes> {
        self.buffer.drain(..self.cursor);
        self.cursor = 0;
        self.expected_arg_count = None;

        std::mem::take(&mut self.args)
    }

    fn parse_header(
        bytes: &[u8],
        header_byte: u8,
        max_header_len: usize,
        expected_error: fn(u8) -> RequestParseError,
        too_long_error: fn(usize) -> RequestParseError,
    ) -> Result<Option<Parsed<usize>>, RequestParseError> {
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

    pub fn into_args(self) -> Vec<Bytes> {
        self.cstrs
    }
}

#[cfg(test)]
impl Request {
    pub fn from_args(cstrs: Vec<Bytes>) -> Request {
        Request { cstrs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    fn read_bytes(bytes: &[u8]) -> Result<Option<Request>, RequestParseError> {
        RequestReader::new(Cursor::new(bytes)).read_next()
    }

    /// Assert that parsing `input` yields a complete request whose bulk strings
    /// match `expected`.
    fn assert_parses(input: &[u8], expected: Vec<Bytes>) {
        let request = read_bytes(input)
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
        let request = read_bytes(input)
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
    fn parse_error(input: &[u8]) -> RequestParseError {
        read_bytes(input).expect_err("parse should fail")
    }

    #[test]
    fn errors_when_array_byte_is_wrong() {
        let err = parse_error(b"&2\r\n$3\r\nGET\r\n$5\r\nMYKEY\r\n");
        assert!(matches!(err, RequestParseError::ExpectedArray(b'&')));
    }

    #[test]
    fn errors_when_array_byte_is_missing() {
        let err = parse_error(b"\r\n$3\r\nGET\r\n$5\r\nMYKEY\r\n");
        assert!(matches!(err, RequestParseError::MissingFirstByte));
    }

    #[test]
    fn errors_when_cstr_byte_is_wrong() {
        let err = parse_error(b"*2\r\n&3\r\nGET\r\n$5\r\nMYKEY\r\n");
        assert!(matches!(err, RequestParseError::ExpectedCString(b'&')));
    }

    #[test]
    fn errors_when_cstr_byte_is_missing() {
        let err = parse_error(b"*2\r\n\r\nGET\r\n$5\r\nMYKEY\r\n");
        assert!(matches!(err, RequestParseError::MissingFirstByte));
    }

    /// Deserialize `input` and assert it ends before a complete request arrives.
    fn assert_unexpected_eof(input: &[u8]) {
        assert!(matches!(
            read_bytes(input),
            Err(RequestParseError::UnexpectedEof)
        ));
    }

    #[test]
    fn array_header_without_crlf_is_unexpected_eof() {
        assert_unexpected_eof(b"*2");
    }

    #[test]
    fn cstr_header_without_crlf_is_unexpected_eof() {
        assert_unexpected_eof(b"*2\r\n$3");
    }

    #[test]
    fn errors_on_missing_crlf_after_cstr_payload() {
        // "GET" is 3 bytes, but it's followed by "XX" instead of "\r\n".
        let err = parse_error(b"*1\r\n$3\r\nGETXX");
        assert!(matches!(err, RequestParseError::MissingCrlf));
    }

    #[test]
    fn errors_on_partial_crlf_after_cstr_payload() {
        // "GET" is 3 bytes, followed by "\rX": has the \r but not the \n.
        let err = parse_error(b"*1\r\n$3\r\nGET\rX");
        assert!(matches!(err, RequestParseError::MissingCrlf));
    }

    #[test]
    fn parses_array_header_split_across_buffer_fills() {
        let reader = BufReader::with_capacity(2, Cursor::new(b"*1\r\n$4\r\nPING\r\n"));
        let mut requests = RequestReader::new(reader);

        let request = requests
            .read_next()
            .expect("parse should succeed")
            .expect("request should be complete");

        assert_eq!(request.cstrs, vec![b"PING".to_vec()]);
    }

    #[test]
    fn completes_request_fed_in_chunks() {
        let reader =
            BufReader::with_capacity(4, Cursor::new(b"*2\r\n$3\r\nGET\r\n$5\r\nMYKEY\r\n"));
        let mut requests = RequestReader::new(reader);

        let request = requests
            .read_next()
            .expect("no error")
            .expect("request should now be complete");

        assert_eq!(request.cstrs, vec![b"GET".to_vec(), b"MYKEY".to_vec()]);
    }

    #[test]
    fn declared_length_too_short_errors() {
        // Header says 3, but the body is "HELLO" (5). We read "HEL", then the
        // trailing check lands on "LO" instead of "\r\n".
        let err = parse_error(b"*1\r\n$3\r\nHELLO\r\n");
        assert!(matches!(err, RequestParseError::MissingCrlf));
    }

    #[test]
    fn declared_length_too_long_within_buffer_errors() {
        // Header says 5, but the body is "HI" (2). We read 5 bytes ("HI\r\nZ",
        // swallowing the real CRLF), then the trailing check lands on "ZZ".
        let err = parse_error(b"*1\r\n$5\r\nHI\r\nZZZ");
        assert!(matches!(err, RequestParseError::MissingCrlf));
    }

    #[test]
    fn declared_length_too_long_past_buffer_is_unexpected_eof() {
        // Header says 5, but only "HI\r\n" follows, so the declared payload runs
        // past the buffer end before EOF.
        assert_unexpected_eof(b"*1\r\n$5\r\nHI\r\n");
    }

    #[test]
    fn partial_array_header_leaves_state_uncommitted() {
        let mut requests = RequestReader::new(Cursor::new(b"*5\r"));

        // "*5\r" — no full CRLF yet, so the array header can't be committed.
        let result = requests.read_next();

        assert!(matches!(result, Err(RequestParseError::UnexpectedEof)));
        assert_eq!(requests.decoder.expected_arg_count, None);
        assert_eq!(requests.decoder.cursor, 0);
        assert_eq!(requests.decoder.buffer, b"*5\r".to_vec());
        assert!(requests.decoder.args.is_empty());
    }

    #[test]
    fn decoder_is_poisoned_after_an_error() {
        let mut requests = RequestReader::new(Cursor::new(b"&2\r\n$3\r\nGET\r\n"));

        let err = requests
            .read_next()
            .expect_err("bad array byte should error");
        assert!(matches!(err, RequestParseError::ExpectedArray(b'&')));

        let err = requests
            .read_next()
            .expect_err("poisoned parser should reject further input");
        assert!(matches!(err, RequestParseError::Poisoned));
    }

    #[test]
    fn validate_eof_succeeds_when_decoder_is_empty() {
        let decoder = RequestDecoder::default();

        assert!(decoder.validate_eof().is_ok());
    }

    #[test]
    fn validate_eof_succeeds_after_complete_request_is_consumed() {
        let mut requests = RequestReader::new(Cursor::new(b"*1\r\n$4\r\nPING\r\n"));
        requests
            .read_next()
            .expect("parse should succeed")
            .expect("request should be complete");

        assert!(requests.decoder.validate_eof().is_ok());
    }

    #[test]
    fn read_next_errors_when_request_ends_incomplete() {
        assert!(matches!(
            read_bytes(b"*2\r\n$3\r\nGET\r\n"),
            Err(RequestParseError::UnexpectedEof)
        ));
    }

    #[test]
    fn validate_eof_reports_poisoned_before_buffered_data() {
        let mut requests = RequestReader::new(Cursor::new(b"&1\r\n"));
        let _ = requests.read_next().expect_err("parse should fail");

        assert!(matches!(
            requests.decoder.validate_eof(),
            Err(RequestParseError::Poisoned)
        ));
    }

    #[test]
    fn full_array_header_commits_state() {
        let mut requests = RequestReader::new(Cursor::new(b"*5\r\n"));

        // "*5\r\n" — a complete array header, but no bulk strings yet.
        let result = requests.read_next();

        assert!(matches!(result, Err(RequestParseError::UnexpectedEof)));
        assert_eq!(requests.decoder.expected_arg_count, Some(5));
        assert_eq!(requests.decoder.cursor, 4); // past "*5\r\n"
        assert_eq!(requests.decoder.buffer, b"*5\r\n".to_vec());
        assert!(requests.decoder.args.is_empty());
    }

    #[test]
    fn good_array_then_bad_cstr_header_state() {
        let mut requests = RequestReader::new(Cursor::new(b"*2\r\n&3\r\nGET\r\n"));

        // Array header is fine, but the first bulk string uses '&' not '$'.
        let err = requests
            .read_next()
            .expect_err("bad cstr byte should error");

        assert!(matches!(err, RequestParseError::ExpectedCString(b'&')));
        assert!(requests.decoder.poisoned);
        assert_eq!(requests.decoder.expected_arg_count, Some(2));
        assert_eq!(requests.decoder.cursor, 4); // advanced past "*2\r\n" only
        assert!(requests.decoder.args.is_empty());
    }

    #[test]
    fn good_array_good_cstr_header_bad_body_state() {
        let mut requests = RequestReader::new(Cursor::new(b"*1\r\n$3\r\nGETXX"));

        // "$3" then "GET" is fine, but the payload is terminated by "XX".
        let err = requests
            .read_next()
            .expect_err("bad cstr terminator should error");

        assert!(matches!(err, RequestParseError::MissingCrlf));
        assert!(requests.decoder.poisoned);
        assert_eq!(requests.decoder.expected_arg_count, Some(1));
        assert_eq!(requests.decoder.cursor, 4);
        assert!(requests.decoder.args.is_empty());
    }

    #[test]
    fn good_array_one_cstr_then_bad_next_cstr_state() {
        let mut requests = RequestReader::new(Cursor::new(b"*2\r\n$3\r\nGET\r\n&5\r\nMYKEY\r\n"));

        // First bulk string "GET" parses cleanly; the second uses '&' not '$'.
        let err = requests
            .read_next()
            .expect_err("bad second cstr byte should error");

        assert!(matches!(err, RequestParseError::ExpectedCString(b'&')));
        assert!(requests.decoder.poisoned);
        assert_eq!(requests.decoder.expected_arg_count, Some(2));
        assert_eq!(requests.decoder.cursor, 13);
        assert_eq!(requests.decoder.args, vec![b"GET".to_vec()]);
    }

    #[test]
    fn read_next_yields_second_buffered_request_without_more_bytes() {
        let reader = Cursor::new(b"*2\r\n$3\r\nGET\r\n$1\r\na\r\n*2\r\n$3\r\nGET\r\n$1\r\nb\r\n");
        let mut requests = RequestReader::new(reader);

        let first = requests
            .read_next()
            .expect("no error")
            .expect("first request should be complete");
        assert_eq!(first.cstrs, vec![b"GET".to_vec(), b"a".to_vec()]);

        let second = requests
            .read_next()
            .expect("no error")
            .expect("second request should already be buffered");
        assert_eq!(second.cstrs, vec![b"GET".to_vec(), b"b".to_vec()]);

        assert!(
            requests.read_next().expect("no error").is_none(),
            "no third request"
        );
    }
}
