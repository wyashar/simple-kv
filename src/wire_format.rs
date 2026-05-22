use std::fmt;

#[derive(Debug)]
#[derive(PartialEq)]
enum Operation {
    Put(Vec<u8>, Vec<u8>),
    Get(Vec<u8>),
    Del(Vec<u8>),
}

enum WireFormat {
    Cmd(Operation),
    SimpleString(String)
}

/*
    op\r\n
    Put\r\n
    5\r\n
    MyKey\r\n
    7\r\n
    MyValue

    op\r\n
    Get\r\n
    5\r\n
    MyKey\r\n

    sstr\r\n
    Hello!\r\n
 */
impl TryFrom<&[u8]> for WireFormat {
    type Error = WireFormatParseError;

    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        let mut pos = 0;

        let first_line = WireFormat::read_line(input, &mut pos)
            .ok_or(WireFormatParseError::InvalidCommandEncoding)?;
        let first_line_str = std::str::from_utf8(first_line)
            .map_err(|_| WireFormatParseError::InvalidCommandEncoding)?;

         match first_line_str {
             "op" => {
                 let result = Operation::try_from(&input[pos..])
                     .map_err(|e| WireFormatParseError::OperationError(e))?;

                 Ok(WireFormat::Cmd(result))
             },
             "sstr" => {
                 let simple_str_bytes = WireFormat::read_line(input, &mut pos)
                     .ok_or(WireFormatParseError::InvalidSimpleStringEncoding)?;
                 let simple_str = std::str::from_utf8(simple_str_bytes)
                     .map_err(|_| WireFormatParseError::InvalidSimpleStringEncoding)?
                     .to_string();

                 Ok(WireFormat::SimpleString(simple_str))
             },
             _ => Err(WireFormatParseError::InvalidCommandEncoding)
         }
    }
}

impl WireFormat {
    fn read_line<'a>(input: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
        let remaining = input.get(*pos..)?;
        let crlf = remaining.windows(2).position(|w| w == b"\r\n")?;
        let line = &remaining[..crlf];
        *pos += crlf + 2;

        Some(line)
    }
}

impl fmt::Display for Operation {
    // Keys and/or Values of Operation variants here are guaranteed to be valid UTF8 here
    // This is b/c TryFrom<&[u8]> for Operation enforces it, and it's the only way to construct this Operation type
    // hence we can use String::from_utf8_lossy here
    // note that it's the same performance as std::str::from_utf8(s).unwrap()
    // because String::from_utf8_lossy(str) returns Cow<str>
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operation::Put(key, value) =>
                write!(
                    f,
                    "Put\r\n{}\r\n{}\r\n{}\r\n{}\r\n",
                    key.len(),
                    String::from_utf8_lossy(key),
                    value.len(),
                    String::from_utf8_lossy(value)
                ),
            Operation::Get(key) =>
                write!(
                    f,
                   "Get\r\n{}\r\n{}\r\n",
                   key.len(),
                   String::from_utf8_lossy(key)
                ),
            Operation::Del(key) =>
                write!(
                    f,
                    "Del\r\n{}\r\n{}\r\n",
                    key.len(),
                    String::from_utf8_lossy(key)
                ),
        }
    }
}

impl fmt::Display for WireFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireFormat::Cmd(op) =>
                write!(
                    f,
                    "op\r\n{}",
                    op
                ),
            WireFormat::SimpleString(s) =>
                write!(
                    f,
                    "sstr\r\n{}\r\n",
                    s
                )
        }
    }
}

impl From<Operation> for Vec<u8> {
    fn from(op: Operation) -> Self {
        op.to_string().into_bytes()
    }
}

impl From<WireFormat> for Vec<u8> {
    fn from(wf: WireFormat) -> Self {
        wf.to_string().into_bytes()
    }
}

#[derive(Debug)]
enum WireFormatParseError {
    InvalidCommandEncoding,
    InvalidSimpleStringEncoding,
    OperationError(OperationParseError)
}

#[derive(Debug, PartialEq)]
enum OperationParseError {
    InvalidOperationEncoding,
    InvalidKeyLenEncoding,
    InvalidKeyEncoding,
    InvalidValueLenEncoding,
    InvalidValueEncoding,
    UnknownOperation,
}

impl TryFrom<&[u8]> for Operation {
    type Error = OperationParseError;
    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        let mut pos = 0;

        let op_line = WireFormat::read_line(input, &mut pos)
            .ok_or(OperationParseError::InvalidOperationEncoding)?;
        let operation_str = std::str::from_utf8(op_line)
            .map_err(|_| OperationParseError::InvalidOperationEncoding)
            .and_then(|s| match s {
                "Put" | "Del" | "Get" => Ok(s),
                _ => Err(OperationParseError::InvalidOperationEncoding)
            })?;

        let key_len_line = WireFormat::read_line(input, &mut pos)
            .ok_or(OperationParseError::InvalidKeyLenEncoding)?;
        let key_len: usize = std::str::from_utf8(key_len_line)
            .map_err(|_| OperationParseError::InvalidKeyLenEncoding)?
            .parse()
            .map_err(|_| OperationParseError::InvalidKeyLenEncoding)?;

        let key_line = WireFormat::read_line(input, &mut pos)
            .ok_or(OperationParseError::InvalidKeyEncoding)?;
        if key_line.len() != key_len {
            return Err(OperationParseError::InvalidKeyEncoding);
        }
        let key = key_line.to_vec();

        match operation_str {
            "Put" => {
                let value_len_line = WireFormat::read_line(input, &mut pos)
                    .ok_or(OperationParseError::InvalidValueLenEncoding)?;
                let value_len: usize = std::str::from_utf8(value_len_line)
                    .map_err(|_| OperationParseError::InvalidValueLenEncoding)?
                    .parse()
                    .map_err(|_| OperationParseError::InvalidValueLenEncoding)?;

                let value_line = WireFormat::read_line(input, &mut pos)
                    .ok_or(OperationParseError::InvalidValueEncoding)?;
                if value_line.len() != value_len {
                    return Err(OperationParseError::InvalidValueEncoding);
                }
                let value = value_line.to_vec();

                if pos != input.len() {
                    return Err(OperationParseError::InvalidOperationEncoding);
                }

                Ok(Operation::Put(key, value))
            }
            "Get" => {
                if pos != input.len() {
                    return Err(OperationParseError::InvalidOperationEncoding);
                }
                Ok(Operation::Get(key))
            }
            "Del" => {
                if pos != input.len() {
                    return Err(OperationParseError::InvalidOperationEncoding);
                }
                Ok(Operation::Del(key))
            }
            _ => unreachable!("all variants of Operation were matched as strs during byte parsing"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_u8_for_operation_bad_operation_bytes() {
        let bad_bytes: &[u8] = b"hello!\r\n".as_slice();
        let actual: Result<Operation, OperationParseError> = bad_bytes.try_into();
        let expected: Result<Operation, OperationParseError> = Err(OperationParseError::InvalidOperationEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_empty_byte_arr() {
        let empty_byte_arr: &[u8] = b"".as_slice();
        let actual: Result<Operation, OperationParseError> = empty_byte_arr.try_into();
        let expected: Result<Operation, OperationParseError> = Err(OperationParseError::InvalidOperationEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_bad_key_len_bytes() {
        let
    }
}