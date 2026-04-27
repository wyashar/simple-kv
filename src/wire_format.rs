use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;
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

    /*
        a3\r\t
        l3\r\t
        PUT\r\t
        l5\r\t
        MyKey\r\t
        l7\r\t
        MyValue\r\t
     */
    pub fn decode(stream: &TcpStream) -> Result<Self, WireFormatParseError> {
        let mut buff: BufReader<&TcpStream> = BufReader::new(stream);

        let mut first_line: String = String::new();
        buff
            .read_line(&mut first_line)
            .map_err(WireFormatParseError::TmpErr)?;

        let msg_len: usize = first_line
            .chars()
            .skip(1)// TODO: we are skipping the type here, will impl and use it later
            .collect::<String>()
            .trim()
            .parse::<usize>()
            .map_err(WireFormatParseError::TmpErr)?;

        let mut data_buffer: Vec<Vec<u8>> = Vec::with_capacity(msg_len);

        for _ in 0..msg_len {
            let mut next_line: String = String::new();
            buff
                .read_line(&mut next_line)
                .map_err(WireFormatParseError::TmpErr)?;

            let data_len: usize = next_line
                .chars()
                .skip(1)
                .collect::<String>()
                .trim()
                .parse::<usize>()
                .map_err(WireFormatParseError::TmpErr)?;

            let mut data: Vec<u8> = vec![0u8; data_len];
            buff
                .read_exact(&mut data)
                .map_err(WireFormatParseError::TmpErr)?;

            let mut clrf: [u8; 2] = [0; 2];
            buff
                .read_exact(&mut clrf)
                .map_err(WireFormatParseError::TmpErr)?;

            data_buffer.push(data);
        }

        let operation: String = String::from_utf8(data_buffer[0]).map_err(|_| WireFormatParseError::TmpErr)?;
        let key: String = String::from_utf8(data_buffer[1]).map_err(|_| WireFormatParseError::TmpErr)?;


        todo!()
    }

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
    TmpErr
}

#[derive(Debug)]
pub enum OperationParseError {
    UnknownOperation(String),
}