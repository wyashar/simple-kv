use std::str::FromStr;

#[derive(Debug)]
pub struct WireFormat {
    pub operation: WireFormatOperation,
    pub key: String,
    pub data: String
}

impl WireFormat {
    fn new(operation: WireFormatOperation, key: String, data: String) -> Self {
        WireFormat { operation, key, data }
    }
}

impl FromStr for WireFormat {
    type Err = WireFormatParseError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = input.split_whitespace().collect();

        let operation = WireFormatOperation::from_str(parts.get(0).ok_or(WireFormatParseError::MissingOperation)?)
            .map_err(WireFormatParseError::InvalidOperation)?;
        let key = parts.get(1).ok_or(WireFormatParseError::MissingKey)?.to_string();
        let data = parts.get(2).ok_or(WireFormatParseError::MissingData)?.to_string();

        if parts.len() > 3 {
            return Err(WireFormatParseError::TooManyParts);
        }

        Ok(WireFormat::new(operation, key, data))
    }
}

#[derive(Debug)]
pub enum WireFormatParseError {
    TooManyParts,
    InvalidOperation(OperationParseError),
    MissingKey,
    MissingOperation,
    MissingData
}

#[derive(Debug)]
pub enum WireFormatOperation {
    Put,
    Get,
    Del
}

#[derive(Debug)]
pub enum OperationParseError {
    UnknownOperation(String)
}


impl FromStr for WireFormatOperation {
    type Err = OperationParseError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "PUT" => Ok(WireFormatOperation::Put),
            "GET" => Ok(WireFormatOperation::Get),
            "DEL" => Ok(WireFormatOperation::Del),
            _ => Err(OperationParseError::UnknownOperation(input.to_string()))
        }
    }
}