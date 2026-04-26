use std::collections::HashMap;

pub struct KvStore {
    map: HashMap<String, Vec<u8>>,
}

pub enum KvStoreResult {
    Stored,
    Found(Vec<u8>),
    Removed(Vec<u8>),
    NotFound
}

impl KvStore {
    pub fn new() -> Self {
        KvStore { map: HashMap::new() }
    }

    pub fn put(&mut self, k: String, v: Vec<u8>) -> KvStoreResult {
        self.map.insert(k, v);
        KvStoreResult::Stored
    }

    pub fn get(&mut self, k: &str) -> KvStoreResult {
        match self.map.get(k) {
            Some(v) => KvStoreResult::Found(v.clone()),
            None => KvStoreResult::NotFound
        }
    }

    pub fn del(&mut self, k: &str) -> KvStoreResult {
        match self.map.remove(k) {
            Some(v) => KvStoreResult::Removed(v),
            None => KvStoreResult::NotFound
        }
    }
}