#[derive(Debug)]
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

        let first_line = read_line(input, &mut pos)
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
                 let simple_str_bytes = read_line(input, &mut pos)
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

#[derive(Debug)]
enum WireFormatParseError {
    InvalidCommandEncoding,
    InvalidSimpleStringEncoding,
    OperationError(OperationParseError)
}

#[derive(Debug)]
enum OperationParseError {
    InvalidOperationEncoding,
    InvalidKeyLenEncoding,
    InvalidKeyEncoding,
    InvalidValueLenEncoding,
    InvalidValueEncoding,
    UnknownOperation,
}

fn read_line<'a>(input: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    let remaining = input.get(*pos..)?;
    let crlf = remaining.windows(2).position(|w| w == b"\r\n")?;
    let line = &remaining[..crlf];
    *pos += crlf + 2;

    Some(line)
}

impl TryFrom<&[u8]> for Operation {
    type Error = OperationParseError;
    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        let mut pos = 0;

        let op_line = read_line(input, &mut pos)
            .ok_or(OperationParseError::InvalidOperationEncoding)?;
        let operation_str = std::str::from_utf8(op_line)
            .map_err(|_| OperationParseError::InvalidOperationEncoding)?;

        let key_len_line = read_line(input, &mut pos)
            .ok_or(OperationParseError::InvalidKeyLenEncoding)?;
        let key_len: usize = std::str::from_utf8(key_len_line)
            .map_err(|_| OperationParseError::InvalidKeyLenEncoding)?
            .parse()
            .map_err(|_| OperationParseError::InvalidKeyLenEncoding)?;

        let key_line = read_line(input, &mut pos)
            .ok_or(OperationParseError::InvalidKeyEncoding)?;
        if key_line.len() != key_len {
            return Err(OperationParseError::InvalidKeyEncoding);
        }
        let key = key_line.to_vec();

        match operation_str {
            "Put" => {
                let value_len_line = read_line(input, &mut pos)
                    .ok_or(OperationParseError::InvalidValueLenEncoding)?;
                let value_len: usize = std::str::from_utf8(value_len_line)
                    .map_err(|_| OperationParseError::InvalidValueLenEncoding)?
                    .parse()
                    .map_err(|_| OperationParseError::InvalidValueLenEncoding)?;

                let value_line = read_line(input, &mut pos)
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
            _ => Err(OperationParseError::UnknownOperation),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_put(key: &[u8], value: &[u8]) -> Vec<u8> {
        format!(
            "Put\r\n{}\r\n{}\r\n{}\r\n{}\r\n",
            key.len(),
            String::from_utf8_lossy(key),
            value.len(),
            String::from_utf8_lossy(value)
        )
        .into_bytes()
    }

    fn make_get(key: &[u8]) -> Vec<u8> {
        format!(
            "Get\r\n{}\r\n{}\r\n",
            key.len(),
            String::from_utf8_lossy(key)
        )
        .into_bytes()
    }

    fn make_del(key: &[u8]) -> Vec<u8> {
        format!(
            "Del\r\n{}\r\n{}\r\n",
            key.len(),
            String::from_utf8_lossy(key)
        )
        .into_bytes()
    }

    fn make_wire_op(op_bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"op\r\n");
        out.extend_from_slice(op_bytes);
        out
    }

    fn make_wire_put(key: &[u8], value: &[u8]) -> Vec<u8> {
        make_wire_op(&make_put(key, value))
    }

    fn make_wire_get(key: &[u8]) -> Vec<u8> {
        make_wire_op(&make_get(key))
    }

    fn make_wire_del(key: &[u8]) -> Vec<u8> {
        make_wire_op(&make_del(key))
    }

    fn make_wire_sstr(s: &str) -> Vec<u8> {
        format!("sstr\r\n{}\r\n", s).into_bytes()
    }

    #[test]
    fn test_wire_format_sstr() {
        let input = make_wire_sstr("Hello!");
        let wf = WireFormat::try_from(input.as_slice()).unwrap();
        match wf {
            WireFormat::SimpleString(s) => {
                assert_eq!(s, "Hello!");
            }
            _ => panic!("Expected WireFormat::SimpleString"),
        }
    }

    #[test]
    fn test_wire_format_sstr_empty() {
        let input = make_wire_sstr("");
        let wf = WireFormat::try_from(input.as_slice()).unwrap();
        match wf {
            WireFormat::SimpleString(s) => {
                assert_eq!(s, "");
            }
            _ => panic!("Expected WireFormat::SimpleString"),
        }
    }

    #[test]
    fn test_wire_format_sstr_missing_content() {
        let input = b"sstr\r\n";
        let result = WireFormat::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(WireFormatParseError::InvalidSimpleStringEncoding)
        ));
    }

    #[test]
    fn test_wire_format_sstr_non_utf8() {
        let input = b"sstr\r\n\xff\xfe\r\n";
        let result = WireFormat::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(WireFormatParseError::InvalidSimpleStringEncoding)
        ));
    }

    #[test]
    fn test_wire_format_put() {
        let input = make_wire_put(b"key", b"value");
        let wf = WireFormat::try_from(input.as_slice()).unwrap();
        match wf {
            WireFormat::Cmd(Operation::Put(k, v)) => {
                assert_eq!(k, b"key");
                assert_eq!(v, b"value");
            }
            _ => panic!("Expected WireFormat::Cmd(Operation::Put)"),
        }
    }

    #[test]
    fn test_wire_format_get() {
        let input = make_wire_get(b"mykey");
        let wf = WireFormat::try_from(input.as_slice()).unwrap();
        match wf {
            WireFormat::Cmd(Operation::Get(k)) => {
                assert_eq!(k, b"mykey");
            }
            _ => panic!("Expected WireFormat::Cmd(Operation::Get)"),
        }
    }

    #[test]
    fn test_wire_format_del() {
        let input = make_wire_del(b"mykey");
        let wf = WireFormat::try_from(input.as_slice()).unwrap();
        match wf {
            WireFormat::Cmd(Operation::Del(k)) => {
                assert_eq!(k, b"mykey");
            }
            _ => panic!("Expected WireFormat::Cmd(Operation::Del)"),
        }
    }

    #[test]
    fn test_wire_format_empty_input() {
        let result = WireFormat::try_from(b"".as_slice());
        assert!(matches!(
            result,
            Err(WireFormatParseError::InvalidCommandEncoding)
        ));
    }

    #[test]
    fn test_wire_format_invalid_first_line() {
        let result = WireFormat::try_from(b"invalid\r\nPut\r\n3\r\nabc\r\n".as_slice());
        assert!(matches!(
            result,
            Err(WireFormatParseError::InvalidCommandEncoding)
        ));
    }

    #[test]
    fn test_wire_format_utf8_error_in_first_line() {
        let result = WireFormat::try_from(b"\xff\xfe\r\nPut\r\n".as_slice());
        assert!(matches!(
            result,
            Err(WireFormatParseError::InvalidCommandEncoding)
        ));
    }

    #[test]
    fn test_wire_format_propagates_operation_error() {
        let input = b"op\r\nBad\r\n3\r\nabc\r\n";
        let result = WireFormat::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(WireFormatParseError::OperationError(OperationParseError::UnknownOperation))
        ));
    }

    #[test]
    fn test_wire_format_propagates_key_len_error() {
        let input = b"op\r\nGet\r\nabc\r\n";
        let result = WireFormat::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(WireFormatParseError::OperationError(OperationParseError::InvalidKeyLenEncoding))
        ));
    }

    #[test]
    fn test_parse_put_operation() {
        let input = make_put(b"hello", b"world");
        let op = Operation::try_from(input.as_slice()).unwrap();
        match op {
            Operation::Put(key, value) => {
                assert_eq!(key, b"hello");
                assert_eq!(value, b"world");
            }
            _ => panic!("Expected Put operation"),
        }
    }

    #[test]
    fn test_parse_put_with_empty_value() {
        let input = make_put(b"key", b"");
        let op = Operation::try_from(input.as_slice()).unwrap();
        match op {
            Operation::Put(key, value) => {
                assert_eq!(key, b"key");
                assert_eq!(value, b"");
            }
            _ => panic!("Expected Put operation"),
        }
    }

    #[test]
    fn test_parse_put_with_binary_key_value() {
        let input = make_put(&[0, 1, 2], &[255, 254, 253]);
        let result = Operation::try_from(input.as_slice());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_get_operation() {
        let input = make_get(b"mykey");
        let op = Operation::try_from(input.as_slice()).unwrap();
        match op {
            Operation::Get(key) => {
                assert_eq!(key, b"mykey");
            }
            _ => panic!("Expected Get operation"),
        }
    }

    #[test]
    fn test_parse_get_with_empty_key() {
        let input = make_get(b"");
        let op = Operation::try_from(input.as_slice()).unwrap();
        match op {
            Operation::Get(key) => {
                assert_eq!(key, b"");
            }
            _ => panic!("Expected Get operation"),
        }
    }

    #[test]
    fn test_parse_del_operation() {
        let input = make_del(b"todelete");
        let op = Operation::try_from(input.as_slice()).unwrap();
        match op {
            Operation::Del(key) => {
                assert_eq!(key, b"todelete");
            }
            _ => panic!("Expected Del operation"),
        }
    }

    #[test]
    fn test_parse_del_with_empty_key() {
        let input = make_del(b"");
        let op = Operation::try_from(input.as_slice()).unwrap();
        match op {
            Operation::Del(key) => {
                assert_eq!(key, b"");
            }
            _ => panic!("Expected Del operation"),
        }
    }

    #[test]
    fn test_unknown_operation() {
        let input = b"Unknown\r\n3\r\nabc\r\n";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(result, Err(OperationParseError::UnknownOperation)));
    }

    #[test]
    fn test_invalid_operation_not_utf8() {
        let input = b"\xff\xfe\r\n3\r\nabc\r\n";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(OperationParseError::InvalidOperationEncoding)
        ));
    }

    #[test]
    fn test_missing_operation_line() {
        let input = b"";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(OperationParseError::InvalidOperationEncoding)
        ));
    }

    #[test]
    fn test_invalid_key_len_not_a_number() {
        let input = b"Get\r\nabc\r\n";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(OperationParseError::InvalidKeyLenEncoding)
        ));
    }

    #[test]
    fn test_invalid_key_len_negative() {
        let input = b"Get\r\n-1\r\n";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(OperationParseError::InvalidKeyLenEncoding)
        ));
    }

    #[test]
    fn test_key_length_mismatch_key_too_short() {
        let input = b"Get\r\n5\r\nabc\r\n";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(result, Err(OperationParseError::InvalidKeyEncoding)));
    }

    #[test]
    fn test_key_length_mismatch_key_too_long() {
        let input = b"Get\r\n1\r\nabcdef\r\n";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(result, Err(OperationParseError::InvalidKeyEncoding)));
    }

    #[test]
    fn test_trailing_data_after_get() {
        let input = b"Get\r\n3\r\nabc\r\nextra";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(OperationParseError::InvalidOperationEncoding)
        ));
    }

    #[test]
    fn test_trailing_data_after_put() {
        let input = b"Put\r\n3\r\nabc\r\n3\r\ndef\r\nextra";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(OperationParseError::InvalidOperationEncoding)
        ));
    }

    #[test]
    fn test_missing_value_for_put() {
        let input = b"Put\r\n3\r\nabc\r\n";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(OperationParseError::InvalidValueLenEncoding)
        ));
    }

    #[test]
    fn test_invalid_value_len_not_a_number() {
        let input = b"Put\r\n3\r\nabc\r\nxyz\r\n";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(
            result,
            Err(OperationParseError::InvalidValueLenEncoding)
        ));
    }

    #[test]
    fn test_value_length_mismatch() {
        let input = b"Put\r\n3\r\nabc\r\n5\r\nval\r\n";
        let result = Operation::try_from(input.as_slice());
        assert!(matches!(result, Err(OperationParseError::InvalidValueEncoding)));
    }

    #[test]
    fn test_missing_crlf_in_operation() {
        let input = b"Get\n3\r\nabc\r\n";
        let result = Operation::try_from(input.as_slice());
        assert!(result.is_err());
    }

    #[test]
    fn test_put_with_large_value() {
        let key = b"large";
        let value = vec![b'x'; 1000];
        let input = make_put(key, &value);
        let op = Operation::try_from(input.as_slice()).unwrap();
        match op {
            Operation::Put(k, v) => {
                assert_eq!(k, key);
                assert_eq!(v, value);
            }
            _ => panic!("Expected Put operation"),
        }
    }

    #[test]
    fn test_read_line_empty_input() {
        let mut pos = 0;
        let result = read_line(b"", &mut pos);
        assert!(result.is_none());
    }

    #[test]
    fn test_read_line_no_crlf() {
        let mut pos = 0;
        let result = read_line(b"hello", &mut pos);
        assert!(result.is_none());
    }

    #[test]
    fn test_read_line_basic() {
        let mut pos = 0;
        let result = read_line(b"hello\r\n", &mut pos);
        assert_eq!(result, Some(b"hello".as_ref()));
        assert_eq!(pos, 7);
    }

    #[test]
    fn test_read_line_multiple() {
        let mut pos = 0;
        let input = b"first\r\nsecond\r\n";
        let line1 = read_line(input, &mut pos);
        assert_eq!(line1, Some(b"first".as_ref()));
        assert_eq!(pos, 7);
        let line2 = read_line(input, &mut pos);
        assert_eq!(line2, Some(b"second".as_ref()));
        assert_eq!(pos, 15);
    }
}