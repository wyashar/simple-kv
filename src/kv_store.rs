use base64::{Engine as _, engine::general_purpose};
use chrono::Utc;
use chrono_tz::US::Eastern;
use log::{Level, debug, info, log_enabled};
use std::collections::HashMap;

pub struct KvStore {
    map: HashMap<Vec<u8>, Vec<u8>>,
}

pub enum KvStoreResult {
    Stored,
    Found(Vec<u8>),
    Removed(Vec<u8>),
    NotFound,
}

impl KvStore {
    pub fn new() -> Self {
        KvStore {
            map: HashMap::new(),
        }
    }

    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> KvStoreResult {
        let timestamp = Utc::now()
            .with_timezone(&Eastern)
            .format("%Y-%m-%d %H:%M:%S %Z");

        if log_enabled!(Level::Debug) {
            let key_base64 = general_purpose::STANDARD.encode(&key);
            let value_base64 = general_purpose::STANDARD.encode(&value);
            match self.map.insert(key, value) {
                Some(old_value) => {
                    let old_value_base64 = general_purpose::STANDARD.encode(&old_value);
                    debug!(
                        "[{}] put key={} value={} replaced old_value={}",
                        timestamp, key_base64, value_base64, old_value_base64
                    );
                }
                None => debug!(
                    "[{}] put key={} value={} (new)",
                    timestamp, key_base64, value_base64
                ),
            }
        } else {
            match self.map.insert(key, value) {
                Some(_) => info!("[{}] put successful (replaced)", timestamp),
                None => info!("[{}] put successful (new)", timestamp),
            }
        }

        KvStoreResult::Stored
    }

    pub fn get(&self, key: &[u8]) -> KvStoreResult {
        let timestamp = Utc::now()
            .with_timezone(&Eastern)
            .format("%Y-%m-%d %H:%M:%S %Z");

        if log_enabled!(Level::Debug) {
            let key_base64 = general_purpose::STANDARD.encode(key);
            match self.map.get(key) {
                Some(value) => {
                    let value_base64 = general_purpose::STANDARD.encode(value);
                    debug!(
                        "[{}] get key={} found value={}",
                        timestamp, key_base64, value_base64
                    );
                    KvStoreResult::Found(value.clone())
                }
                None => {
                    debug!("[{}] get key={} not found", timestamp, key_base64);
                    KvStoreResult::NotFound
                }
            }
        } else {
            match self.map.get(key) {
                Some(value) => {
                    info!("[{}] get successful (found)", timestamp);
                    KvStoreResult::Found(value.clone())
                }
                None => {
                    info!("[{}] get not found", timestamp);
                    KvStoreResult::NotFound
                }
            }
        }
    }

    pub fn del(&mut self, key: &[u8]) -> KvStoreResult {
        let timestamp = Utc::now()
            .with_timezone(&Eastern)
            .format("%Y-%m-%d %H:%M:%S %Z");

        if log_enabled!(Level::Debug) {
            let key_base64 = general_purpose::STANDARD.encode(key);
            match self.map.remove(key) {
                Some(removed_value) => {
                    let removed_value_base64 = general_purpose::STANDARD.encode(&removed_value);
                    debug!(
                        "[{}] del key={} removed value={}",
                        timestamp, key_base64, removed_value_base64
                    );
                    KvStoreResult::Removed(removed_value)
                }
                None => {
                    debug!("[{}] del key={} not found", timestamp, key_base64);
                    KvStoreResult::NotFound
                }
            }
        } else {
            match self.map.remove(key) {
                Some(removed_value) => {
                    info!("[{}] del successful (removed)", timestamp);
                    KvStoreResult::Removed(removed_value)
                }
                None => {
                    info!("[{}] del not found", timestamp);
                    KvStoreResult::NotFound
                }
            }
        }
    }
}
