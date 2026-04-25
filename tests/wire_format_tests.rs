use simple_kv::wire_format::{WireFormat, WireFormatParseError, WireFormatOperation};
use std::str::FromStr;

#[test]
fn test_wire_format_put_valid() {
    let input = "PUT key1 value1";
    let result = WireFormat::from_str(input).unwrap();
    
    assert_eq!(result.operation, WireFormatOperation::Put);
    assert_eq!(result.key, "key1");
    assert_eq!(result.data, Some("value1".to_string()));
}

#[test]
fn test_wire_format_get_valid() {
    let input = "GET mykey";
    let result = WireFormat::from_str(input).unwrap();
    
    assert_eq!(result.operation, WireFormatOperation::Get);
    assert_eq!(result.key, "mykey");
    assert_eq!(result.data, None);
}

#[test]
fn test_wire_format_del_valid() {
    let input = "DEL somekey";
    let result = WireFormat::from_str(input).unwrap();
    
    assert_eq!(result.operation, WireFormatOperation::Del);
    assert_eq!(result.key, "somekey");
    assert_eq!(result.data, None);
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
    let put_lower = WireFormatOperation::from_str("put").unwrap();
    let put_upper = WireFormatOperation::from_str("PUT").unwrap();
    let put_mixed = WireFormatOperation::from_str("Put").unwrap();
    
    assert_eq!(put_lower, put_upper);
    assert_eq!(put_upper, put_mixed);
}