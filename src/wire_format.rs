#[derive(Debug)]
enum Operation {
    Put(Vec<u8>, Vec<u8>),
    Get(Vec<u8>),
    Del(Vec<u8>),
}

enum WireFormat {
    Op(Operation),
}

impl TryFrom<&[u8]> for WireFormat {
    type Error = ();

    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        todo!()
    }
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
}