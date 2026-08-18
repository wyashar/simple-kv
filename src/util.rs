pub type Bytes = Vec<u8>;

pub const CRLF: &[u8; 2] = b"\r\n";
pub const CSTRING_BYTE: u8 = b'$';
pub const MAX_COMPLEX_STRING_LENGTH: usize = 512 * 1024 * 1024; // 512 MB

pub struct Parsed<T> {
    pub data: T,
    pub num_bytes_parsed: usize,
}

impl<T> Parsed<T> {
    pub fn new(data: T, num_bytes_parsed: usize) -> Parsed<T> {
        Parsed {
            data,
            num_bytes_parsed,
        }
    }
}

/// Find the first CRLF-terminated line in `bytes`, returning the line contents
/// (without the trailing CRLF) and the number of bytes consumed (including it).
pub fn parse_line(bytes: &[u8]) -> Option<Parsed<&[u8]>> {
    let crlf_pos = bytes.windows(2).position(|w| w == CRLF)?;
    let data = &bytes[..crlf_pos];
    let num_bytes_parsed = crlf_pos + 2;

    Some(Parsed::new(data, num_bytes_parsed))
}
