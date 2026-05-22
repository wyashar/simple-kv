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
    TooManyParts
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
                _ => Err(OperationParseError::UnknownOperation)
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
                    return Err(OperationParseError::TooManyParts);
                }

                Ok(Operation::Put(key, value))
            }
            "Get" => {
                if pos != input.len() {
                    return Err(OperationParseError::TooManyParts);
                }
                Ok(Operation::Get(key))
            }
            "Del" => {
                if pos != input.len() {
                    return Err(OperationParseError::TooManyParts);
                }
                Ok(Operation::Del(key))
            }
            _ => unreachable!("all variants of Operation were matched as strs during byte parsing"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::wire_format::Operation::{Del, Get, Put};
    use super::*;

    #[test]
    fn try_from_u8_for_operation_bad_operation_bytes() {
        let bad_bytes: &[u8] = b"hello!\r\n".as_slice();
        let actual: Result<Operation, OperationParseError> = bad_bytes.try_into();
        let expected: Result<Operation, OperationParseError> = Err(OperationParseError::UnknownOperation);

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
    fn try_from_u8_for_operation_bad_operator() {
        let byte_arry: &[u8] = b"InvalidOperation\r\n";
        let actual: Result<Operation, OperationParseError> = byte_arry.try_into();
        let expected: Result<Operation, OperationParseError> = Err(OperationParseError::UnknownOperation);

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_bad_key_len() {
        let byte_arr: &[u8] = b"Put\r\nHello\r\n";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();
        let expected: Result<Operation, OperationParseError> = Err(OperationParseError::InvalidKeyLenEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_mismatch_key_len() {
        let byte_arr: &[u8] = b"Get\r\n5\r\nNotFive\r\n";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();
        let expected: Result<Operation, OperationParseError> = Err(OperationParseError::InvalidKeyEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_bad_value_len_encoding() {
        let byte_arr: &[u8] = b"Put\r\n5\r\n12345\r\nInvalidLen";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();
        let expected: Result<Operation, OperationParseError> = Err(OperationParseError::InvalidValueLenEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_value_len_mismatch() {
        let byte_arr: &[u8] = b"Put\r\n5\r\n12345\r\n6\r\nSeven";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();
        let expected: Result<Operation, OperationParseError> = Err(OperationParseError::InvalidValueEncoding);

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_byte_arr_too_long() {
        let byte_arr: &[u8] = b"Put\r\n5\r\n12345\r\n6\r\nSixSix\r\nEvenMore";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();
        let expected: Result<Operation, OperationParseError> = Err(OperationParseError::TooManyParts);

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_byte_arr_too_long_2() {
        let byte_arr: &[u8] = b"Get\r\n5\r\n12345\r\n6";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();
        let expected: Result<Operation, OperationParseError> = Err(OperationParseError::TooManyParts);

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_put_valid() {
        let byte_arr: &[u8] = b"Put\r\n6\r\nKey123\r\n7\r\nValue12\r\n";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();

        let key_bytes: Vec<u8> = b"Key123".to_vec();
        let value_bytes: Vec<u8> = b"Value12".to_vec();
        let expected: Result<Operation, OperationParseError> = Ok(Put(key_bytes, value_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_get_valid() {
        let byte_arr: &[u8] = b"Get\r\n17\r\nDrakeIsABadArtist\r\n";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();

        let key_bytes: Vec<u8> = b"DrakeIsABadArtist".to_vec();
        let expected: Result<Operation, OperationParseError> = Ok(Get(key_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_del_valid() {
        let byte_arr: &[u8] = b"Del\r\n4\r\nTree\r\n";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();

        let key_bytes: Vec<u8> = b"Tree".to_vec();
        let expected: Result<Operation, OperationParseError> = Ok(Del(key_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_non_byte_safe_string() {
        // 𝕳𝖊𝖑𝖑𝖔 Wörld! ñoño 日本語 中文 한국어 العربية עברית ℃ ™ © ® € £ ¥ ✓ ← ↑ → ↓ ♠ ♣ ♥ ♦
        let byte_arr: &[u8] = b"Put\r\n5\r\nMyKey\r\n148\r\n\xf0\x9d\x95\xb3\xf0\x9d\x96\x8a\xf0\x9d\x96\x91\xf0\x9d\x96\x91\xf0\x9d\x96\x94 W\xc3\xb6rld! \xc3\xb1o\xc3\xb1o \xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e \xe4\xb8\xad\xe6\x96\x87 \xed\x95\x9c\xea\xb5\xad\xec\x96\xb4 \xd8\xa7\xd9\x84\xd8\xb9\xd8\xb1\xd8\xa8\xd9\x8a\xd8\xa9 \xd7\xa2\xd7\x91\xd7\xa8\xd7\x99\xd7\xaa \xe2\x84\x83 \xe2\x84\xa2 \xc2\xa9 \xc2\xae \xe2\x82\xac \xc2\xa3 \xc2\xa5 \xe2\x9c\x93 \xe2\x86\x90 \xe2\x86\x91 \xe2\x86\x92 \xe2\x86\x93 \xe2\x99\xa0 \xe2\x99\xa3 \xe2\x99\xa5 \xe2\x99\xa6\r\n";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();

        let key_bytes: Vec<u8> = b"MyKey".to_vec();
        let value_bytes: Vec<u8> = b"\xf0\x9d\x95\xb3\xf0\x9d\x96\x8a\xf0\x9d\x96\x91\xf0\x9d\x96\x91\xf0\x9d\x96\x94 W\xc3\xb6rld! \xc3\xb1o\xc3\xb1o \xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e \xe4\xb8\xad\xe6\x96\x87 \xed\x95\x9c\xea\xb5\xad\xec\x96\xb4 \xd8\xa7\xd9\x84\xd8\xb9\xd8\xb1\xd8\xa8\xd9\x8a\xd8\xa9 \xd7\xa2\xd7\x91\xd7\xa8\xd7\x99\xd7\xaa \xe2\x84\x83 \xe2\x84\xa2 \xc2\xa9 \xc2\xae \xe2\x82\xac \xc2\xa3 \xc2\xa5 \xe2\x9c\x93 \xe2\x86\x90 \xe2\x86\x91 \xe2\x86\x92 \xe2\x86\x93 \xe2\x99\xa0 \xe2\x99\xa3 \xe2\x99\xa5 \xe2\x99\xa6".to_vec();
        let expected: Result<Operation, OperationParseError> = Ok(Put(key_bytes, value_bytes));

        assert_eq!(actual, expected);
    }

    #[test]
    fn try_from_u8_for_operation_zalgo_key_del() {
        // Z̸̡̢̛̛̺̙͔̮͍̺̘̣̺̖͚̬̖͙͍̣̘̤̟̪̦̬͕̖̩̠͕͖̤̟̱̙̼̳͙̬̦̳͉̦̻̙̥̗̘͇͍̤̫̫͎̱̰͈̺̜̤͔̀͐͌̀͂͗̈́̌̅̊͑̋̒͒͊̀̓̏͊͌̎̈́̀̈́͘͘͜͠͝͠͝ͅͅą̷̧̧̢̛̛̛̹̟͎͉̝̩̬͚͖̝̩̱̩͕͔̖͇̘͇̗̯̙̣͙̮̙̗̹̺͕̱̰̱̲̬̞̤̳̹͍̝͕͑͒̒̐̓̐̃̃̽̏̾̆̋͌͌̒́̋̅́̏͒͗̎̒̔̑̀͑̿̄͑͑̿̈́͋̕̕͜͠͠ͅl̴͇̜̣̬̝̮̭̟͇͚̖͈͎͚͕͔͕͚̹̺̲̙̺̹͂͌̓̂̀̾̅̀̉̒̃̑̋̅͘̕͜͝g̷͉̲̙̥̜̟͔͓̰̯͇̮͎͓̈͛̒̊͐͛̃̊̆̂̎̈́͌͆̀͊̌̃̍̃̿͝͝ͅo̷̧̤̤̮̟̮̻̟̪̱̬͎̟̙̝͔͙̲̎̒̅̔̉͗̈́͛͊̈͆̀̈́͊͑̎̐̌̒͆͊̕̕̚
        let byte_arr: &[u8] = b"Del\r\n644\r\n\x5a\xcc\xb8\xcc\xa1\xcc\xa2\xcc\x9b\xcc\x9b\xcc\xba\xcc\x99\xcd\x94\xcc\xae\xcd\x8d\xcc\xba\xcc\x98\xcc\xa3\xcc\xba\xcc\x96\xcd\x9a\xcc\xac\xcc\x96\xcd\x99\xcd\x8d\xcc\xa3\xcc\x98\xcc\xa4\xcc\x9f\xcc\xaa\xcc\xa6\xcc\xac\xcd\x95\xcc\x96\xcc\xa9\xcc\xa0\xcd\x95\xcd\x96\xcc\xa4\xcc\x9f\xcc\xb1\xcc\x99\xcc\xbc\xcc\xb3\xcd\x99\xcc\xac\xcc\xa6\xcc\xb3\xcd\x89\xcc\xa6\xcc\xbb\xcc\x99\xcc\xa5\xcc\x97\xcc\x98\xcd\x87\xcd\x8d\xcc\xa4\xcc\xab\xcc\xab\xcd\x8e\xcc\xb1\xcc\xb0\xcd\x88\xcc\xba\xcc\x9c\xcc\xa4\xcd\x94\xcc\x80\xcd\x90\xcd\x8c\xcc\x80\xcd\x82\xcd\x97\xcc\x88\xcc\x81\xcc\x8c\xcc\x85\xcc\x8a\xcd\x91\xcc\x8b\xcc\x92\xcd\x92\xcd\x8a\xcc\x80\xcc\x93\xcc\x8f\xcd\x8a\xcd\x8c\xcc\x8e\xcc\x88\xcc\x81\xcc\x80\xcc\x88\xcc\x81\xcd\x98\xcd\x98\xcd\x9c\xcd\xa0\xcd\x9d\xcd\xa0\xcd\x9d\xcd\x85\xcd\x85\xc4\x85\xcc\xb7\xcc\xa7\xcc\xa7\xcc\xa2\xcc\x9b\xcc\x9b\xcc\x9b\xcc\xb9\xcc\x9f\xcd\x8e\xcd\x89\xcc\x9d\xcc\xa9\xcc\xac\xcd\x9a\xcd\x96\xcc\x9d\xcc\xa9\xcc\xb1\xcc\xa9\xcd\x95\xcd\x94\xcc\x96\xcd\x87\xcc\x98\xcd\x87\xcc\x97\xcc\xaf\xcc\x99\xcc\xa3\xcd\x99\xcc\xae\xcc\x99\xcc\x97\xcc\xb9\xcc\xba\xcd\x95\xcc\xb1\xcc\xb0\xcc\xb1\xcc\xb2\xcc\xac\xcc\x9e\xcc\xa4\xcc\xb3\xcc\xb9\xcd\x8d\xcc\x9d\xcd\x95\xcd\x91\xcd\x92\xcc\x92\xcc\x90\xcc\x93\xcc\x90\xcc\x83\xcc\x83\xcc\xbd\xcc\x8f\xcc\xbe\xcc\x86\xcc\x8b\xcd\x8c\xcd\x8c\xcc\x92\xcc\x81\xcc\x8b\xcc\x85\xcc\x81\xcc\x8f\xcd\x92\xcd\x97\xcc\x8e\xcc\x92\xcc\x94\xcc\x91\xcc\x80\xcd\x91\xcc\xbf\xcc\x84\xcd\x91\xcd\x91\xcc\xbf\xcc\x88\xcc\x81\xcd\x8b\xcc\x95\xcc\x95\xcd\x9c\xcd\xa0\xcd\xa0\xcd\x85\x6c\xcc\xb4\xcd\x87\xcc\x9c\xcc\xa3\xcc\xac\xcc\x9d\xcc\xae\xcc\xad\xcc\x9f\xcd\x87\xcd\x9a\xcc\x96\xcd\x88\xcd\x8e\xcd\x9a\xcd\x95\xcd\x94\xcd\x95\xcd\x9a\xcc\xb9\xcc\xba\xcc\xb2\xcc\x99\xcc\xba\xcc\xb9\xcd\x82\xcd\x8c\xcc\x93\xcc\x82\xcc\x80\xcc\xbe\xcc\x85\xcc\x80\xcc\x89\xcc\x92\xcc\x83\xcc\x91\xcc\x8b\xcc\x85\xcd\x98\xcc\x95\xcd\x9c\xcd\x9d\x67\xcc\xb7\xcd\x89\xcc\xb2\xcc\x99\xcc\xa5\xcc\x9c\xcc\x9f\xcd\x94\xcd\x93\xcc\xb0\xcc\xaf\xcd\x87\xcc\xae\xcd\x8e\xcd\x93\xcc\x88\xcd\x9b\xcc\x92\xcc\x8a\xcd\x90\xcd\x9b\xcc\x83\xcc\x8a\xcc\x86\xcc\x82\xcc\x8e\xcc\x88\xcc\x81\xcd\x8c\xcd\x86\xcc\x80\xcd\x8a\xcc\x8c\xcc\x83\xcc\x8d\xcc\x83\xcc\xbf\xcd\x9d\xcd\x9d\xcd\x85\x6f\xcc\xb7\xcc\xa7\xcc\xa4\xcc\xa4\xcc\xae\xcc\x9f\xcc\xae\xcc\xbb\xcc\x9f\xcc\xaa\xcc\xb1\xcc\xac\xcd\x8e\xcc\x9f\xcc\x99\xcc\x9d\xcd\x94\xcd\x99\xcc\xb2\xcc\x8e\xcc\x92\xcc\x85\xcc\x94\xcc\x89\xcd\x97\xcc\x88\xcc\x81\xcd\x9b\xcd\x8a\xcc\x88\xcd\x86\xcc\x80\xcc\x88\xcc\x81\xcd\x8a\xcd\x91\xcc\x8e\xcc\x90\xcc\x8c\xcc\x92\xcd\x86\xcd\x8a\xcc\x95\xcc\x95\xcc\x9a\r\n";
        let actual: Result<Operation, OperationParseError> = byte_arr.try_into();

        let key_bytes: Vec<u8> = b"\x5a\xcc\xb8\xcc\xa1\xcc\xa2\xcc\x9b\xcc\x9b\xcc\xba\xcc\x99\xcd\x94\xcc\xae\xcd\x8d\xcc\xba\xcc\x98\xcc\xa3\xcc\xba\xcc\x96\xcd\x9a\xcc\xac\xcc\x96\xcd\x99\xcd\x8d\xcc\xa3\xcc\x98\xcc\xa4\xcc\x9f\xcc\xaa\xcc\xa6\xcc\xac\xcd\x95\xcc\x96\xcc\xa9\xcc\xa0\xcd\x95\xcd\x96\xcc\xa4\xcc\x9f\xcc\xb1\xcc\x99\xcc\xbc\xcc\xb3\xcd\x99\xcc\xac\xcc\xa6\xcc\xb3\xcd\x89\xcc\xa6\xcc\xbb\xcc\x99\xcc\xa5\xcc\x97\xcc\x98\xcd\x87\xcd\x8d\xcc\xa4\xcc\xab\xcc\xab\xcd\x8e\xcc\xb1\xcc\xb0\xcd\x88\xcc\xba\xcc\x9c\xcc\xa4\xcd\x94\xcc\x80\xcd\x90\xcd\x8c\xcc\x80\xcd\x82\xcd\x97\xcc\x88\xcc\x81\xcc\x8c\xcc\x85\xcc\x8a\xcd\x91\xcc\x8b\xcc\x92\xcd\x92\xcd\x8a\xcc\x80\xcc\x93\xcc\x8f\xcd\x8a\xcd\x8c\xcc\x8e\xcc\x88\xcc\x81\xcc\x80\xcc\x88\xcc\x81\xcd\x98\xcd\x98\xcd\x9c\xcd\xa0\xcd\x9d\xcd\xa0\xcd\x9d\xcd\x85\xcd\x85\xc4\x85\xcc\xb7\xcc\xa7\xcc\xa7\xcc\xa2\xcc\x9b\xcc\x9b\xcc\x9b\xcc\xb9\xcc\x9f\xcd\x8e\xcd\x89\xcc\x9d\xcc\xa9\xcc\xac\xcd\x9a\xcd\x96\xcc\x9d\xcc\xa9\xcc\xb1\xcc\xa9\xcd\x95\xcd\x94\xcc\x96\xcd\x87\xcc\x98\xcd\x87\xcc\x97\xcc\xaf\xcc\x99\xcc\xa3\xcd\x99\xcc\xae\xcc\x99\xcc\x97\xcc\xb9\xcc\xba\xcd\x95\xcc\xb1\xcc\xb0\xcc\xb1\xcc\xb2\xcc\xac\xcc\x9e\xcc\xa4\xcc\xb3\xcc\xb9\xcd\x8d\xcc\x9d\xcd\x95\xcd\x91\xcd\x92\xcc\x92\xcc\x90\xcc\x93\xcc\x90\xcc\x83\xcc\x83\xcc\xbd\xcc\x8f\xcc\xbe\xcc\x86\xcc\x8b\xcd\x8c\xcd\x8c\xcc\x92\xcc\x81\xcc\x8b\xcc\x85\xcc\x81\xcc\x8f\xcd\x92\xcd\x97\xcc\x8e\xcc\x92\xcc\x94\xcc\x91\xcc\x80\xcd\x91\xcc\xbf\xcc\x84\xcd\x91\xcd\x91\xcc\xbf\xcc\x88\xcc\x81\xcd\x8b\xcc\x95\xcc\x95\xcd\x9c\xcd\xa0\xcd\xa0\xcd\x85\x6c\xcc\xb4\xcd\x87\xcc\x9c\xcc\xa3\xcc\xac\xcc\x9d\xcc\xae\xcc\xad\xcc\x9f\xcd\x87\xcd\x9a\xcc\x96\xcd\x88\xcd\x8e\xcd\x9a\xcd\x95\xcd\x94\xcd\x95\xcd\x9a\xcc\xb9\xcc\xba\xcc\xb2\xcc\x99\xcc\xba\xcc\xb9\xcd\x82\xcd\x8c\xcc\x93\xcc\x82\xcc\x80\xcc\xbe\xcc\x85\xcc\x80\xcc\x89\xcc\x92\xcc\x83\xcc\x91\xcc\x8b\xcc\x85\xcd\x98\xcc\x95\xcd\x9c\xcd\x9d\x67\xcc\xb7\xcd\x89\xcc\xb2\xcc\x99\xcc\xa5\xcc\x9c\xcc\x9f\xcd\x94\xcd\x93\xcc\xb0\xcc\xaf\xcd\x87\xcc\xae\xcd\x8e\xcd\x93\xcc\x88\xcd\x9b\xcc\x92\xcc\x8a\xcd\x90\xcd\x9b\xcc\x83\xcc\x8a\xcc\x86\xcc\x82\xcc\x8e\xcc\x88\xcc\x81\xcd\x8c\xcd\x86\xcc\x80\xcd\x8a\xcc\x8c\xcc\x83\xcc\x8d\xcc\x83\xcc\xbf\xcd\x9d\xcd\x9d\xcd\x85\x6f\xcc\xb7\xcc\xa7\xcc\xa4\xcc\xa4\xcc\xae\xcc\x9f\xcc\xae\xcc\xbb\xcc\x9f\xcc\xaa\xcc\xb1\xcc\xac\xcd\x8e\xcc\x9f\xcc\x99\xcc\x9d\xcd\x94\xcd\x99\xcc\xb2\xcc\x8e\xcc\x92\xcc\x85\xcc\x94\xcc\x89\xcd\x97\xcc\x88\xcc\x81\xcd\x9b\xcd\x8a\xcc\x88\xcd\x86\xcc\x80\xcc\x88\xcc\x81\xcd\x8a\xcd\x91\xcc\x8e\xcc\x90\xcc\x8c\xcc\x92\xcd\x86\xcd\x8a\xcc\x95\xcc\x95\xcc\x9a".to_vec();
        let expected: Result<Operation, OperationParseError> = Ok(Del(key_bytes));

        assert_eq!(actual, expected);
    }
}