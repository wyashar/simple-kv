use std::str::FromStr;

#[derive(Debug, Eq, PartialEq)]
pub struct WireFormat {
    pub operation: WireFormatOperation,
    pub key: String,
    pub data: Option<String>
}

impl FromStr for WireFormat {
    type Err = WireFormatParseError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = input.split_whitespace().collect();

        let operation = WireFormatOperation::from_str(parts.get(0).ok_or(WireFormatParseError::MissingOperation)?)
            .map_err(WireFormatParseError::InvalidOperation)?;
        let key = parts.get(1).ok_or(WireFormatParseError::MissingKey)?.to_string();

        Ok(match operation {
            WireFormatOperation::Put => {
                let data = parts.get(2).ok_or(WireFormatParseError::MissingData)?.to_string();
                if parts.len() > 3 {
                    return Err(WireFormatParseError::TooManyParts);
                }
                WireFormat { operation, key, data: Some(data) }
            },
            WireFormatOperation::Del | WireFormatOperation::Get => {
                if parts.len() > 2 {
                    return Err(WireFormatParseError::TooManyParts);
                }
                WireFormat { operation, key, data: None }
            }
        })
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

#[derive(Debug, Eq, PartialEq)]
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
        match input.to_uppercase().as_str() {
            "PUT" => Ok(WireFormatOperation::Put),
            "GET" => Ok(WireFormatOperation::Get),
            "DEL" => Ok(WireFormatOperation::Del),
            _ => Err(OperationParseError::UnknownOperation(input.to_string()))
        }
    }
}