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
        self.map.insert(key, value);
        KvStoreResult::Stored
    }

    pub fn get(&self, key: &[u8]) -> KvStoreResult {
        match self.map.get(key) {
            Some(value) => KvStoreResult::Found(value.clone()),
            None => KvStoreResult::NotFound,
        }
    }

    pub fn del(&mut self, key: &[u8]) -> KvStoreResult {
        match self.map.remove(key) {
            Some(removed_value) => KvStoreResult::Removed(removed_value),
            None => KvStoreResult::NotFound,
        }
    }
}
