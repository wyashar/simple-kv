use simple_kv::wire_format::{WireFormat, WireFormatParseError, WireFormatOperation};
use std::str::FromStr;

#[test]
fn test_wire_format_put_valid() {
    let input = "PUT key1 dmFsdWUx"; // "value1" in base64
    let result = WireFormat::from_str(input).unwrap();
    
    assert_eq!(result.key, "key1");
    assert_eq!(result.operation, WireFormatOperation::Put(b"value1".to_vec()));
}

#[test]
fn test_wire_format_get_valid() {
    let input = "GET mykey";
    let result = WireFormat::from_str(input).unwrap();
    
    assert_eq!(result.operation, WireFormatOperation::Get);
    assert_eq!(result.key, "mykey");
}

#[test]
fn test_wire_format_del_valid() {
    let input = "DEL somekey";
    let result = WireFormat::from_str(input).unwrap();
    
    assert_eq!(result.operation, WireFormatOperation::Del);
    assert_eq!(result.key, "somekey");
}

#[test]
fn test_wire_format_put_missing_data() {
    let input = "PUT key1";
    let result = WireFormat::from_str(input);
    assert!(matches!(result, Err(WireFormatParseError::MissingData)));
}

#[test]
fn test_wire_format_get_too_many_parts() {
    let input = "GET key1 extra";
    let result = WireFormat::from_str(input);
    assert!(matches!(result, Err(WireFormatParseError::TooManyParts)));
}

#[test]
fn test_wire_format_del_too_many_parts() {
    let input = "DEL key1 extra";
    let result = WireFormat::from_str(input);
    assert!(matches!(result, Err(WireFormatParseError::TooManyParts)));
}

#[test]
fn test_wire_format_put_too_many_parts() {
    let input = "PUT key1 data extra";
    let result = WireFormat::from_str(input);
    assert!(matches!(result, Err(WireFormatParseError::TooManyParts)));
}

#[test]
fn test_wire_format_missing_operation() {
    let input = "";
    let result = WireFormat::from_str(input);
    assert!(matches!(result, Err(WireFormatParseError::MissingOperation)));
}

#[test]
fn test_wire_format_missing_key() {
    let input = "PUT";
    let result = WireFormat::from_str(input);
    assert!(matches!(result, Err(WireFormatParseError::MissingKey)));
}

#[test]
fn test_wire_format_invalid_operation() {
    let input = "INVALID key1";
    let result = WireFormat::from_str(input);
    assert!(matches!(result, Err(WireFormatParseError::InvalidOperation(_))));
}

#[test]
fn test_wire_format_operation_case_insensitive() {
    let put_lower = WireFormat::from_str("put key dmFsdWUx").unwrap(); // "value1" base64
    let put_upper = WireFormat::from_str("PUT key dmFsdWUx").unwrap();
    let put_mixed = WireFormat::from_str("Put key dmFsdWUx").unwrap();
    
    assert_eq!(put_lower.operation, put_upper.operation);
    assert_eq!(put_upper.operation, put_mixed.operation);
}