use std::str::FromStr;
use crate::wire_format::WireFormatParseError::BadDataFormat;
use base64::{Engine as _, engine::general_purpose};
use crate::kv_store::KvStore;
use crate::kv_store::KvStoreResult;

#[derive(Debug, Eq, PartialEq)]
pub struct WireFormat {
    pub operation: WireFormatOperation,
    pub key: String,
}

impl WireFormat {
    pub fn apply(self, store: &mut KvStore) -> KvStoreResult {
        match self.operation {
            WireFormatOperation::Put(data) => store.put(self.key, data),
            WireFormatOperation::Get => store.get(&self.key),
            WireFormatOperation::Del => store.del(&self.key),
        }
    }
}

impl FromStr for WireFormat {
    type Err = WireFormatParseError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = input.split_whitespace().collect();

        let op_str = parts.get(0).ok_or(WireFormatParseError::MissingOperation)?;
        let key = parts.get(1).ok_or(WireFormatParseError::MissingKey)?.to_string();

        let operation = match op_str.to_uppercase().as_str() {
            "PUT" => {
                let data = general_purpose::STANDARD
                    .decode(parts.get(2).ok_or(WireFormatParseError::MissingData)?)
                    .map_err(|_| BadDataFormat)?;
                if parts.len() > 3 {
                    return Err(WireFormatParseError::TooManyParts);
                }
                WireFormatOperation::Put(data)
            },
            "GET" => {
                if parts.len() > 2 { return Err(WireFormatParseError::TooManyParts); }
                WireFormatOperation::Get
            },
            "DEL" => {
                if parts.len() > 2 { return Err(WireFormatParseError::TooManyParts); }
                WireFormatOperation::Del
            },
            _ => return Err(WireFormatParseError::InvalidOperation(
                OperationParseError::UnknownOperation(op_str.to_string())
            )),
        };

        Ok(WireFormat { operation, key })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum WireFormatOperation {
    Put(Vec<u8>),
    Get,
    Del,
}

#[derive(Debug)]
pub enum WireFormatParseError {
    TooManyParts,
    InvalidOperation(OperationParseError),
    MissingKey,
    MissingOperation,
    MissingData,
    BadDataFormat,
}

#[derive(Debug)]
pub enum OperationParseError {
    UnknownOperation(String),
}