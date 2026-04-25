use simple_kv::{Config, config::ConfigError};

#[test]
fn test_config_new_success() {
    unsafe {
        std::env::set_var("LISTENER_ADDRESS", "127.0.0.1");
        std::env::set_var("LISTENER_PORT", "8080");
    }
    
    let config = Config::new().unwrap();
    assert_eq!(config.listener_address, "127.0.0.1");
    assert_eq!(config.listener_port, 8080);
    
    unsafe {
        std::env::remove_var("LISTENER_ADDRESS");
        std::env::remove_var("LISTENER_PORT");
    }
}

#[test]
fn test_config_missing_listener_address() {
    unsafe {
        std::env::remove_var("LISTENER_ADDRESS");
        std::env::set_var("LISTENER_PORT", "8080");
    }
    
    let result = Config::new();
    assert!(matches!(result, Err(ConfigError::MissingListenerAddress)));
    
    unsafe { std::env::remove_var("LISTENER_PORT"); }
}

#[test]
fn test_config_missing_listener_port() {
    unsafe {
        std::env::set_var("LISTENER_ADDRESS", "127.0.0.1");
        std::env::remove_var("LISTENER_PORT");
    }
    
    let result = Config::new();
    assert!(matches!(result, Err(ConfigError::MissingListenerPort)));
    
    unsafe { std::env::remove_var("LISTENER_ADDRESS"); }
}

#[test]
fn test_config_invalid_port() {
    unsafe {
        std::env::set_var("LISTENER_ADDRESS", "127.0.0.1");
        std::env::set_var("LISTENER_PORT", "not_a_port");
    }
    
    let result = Config::new();
    assert!(matches!(result, Err(ConfigError::InvalidPort(_))));
    
    unsafe {
        std::env::remove_var("LISTENER_ADDRESS");
        std::env::remove_var("LISTENER_PORT");
    }
}