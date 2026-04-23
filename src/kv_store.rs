use std::collections::HashMap;

pub struct KvStore {
    map: HashMap<String, String>,
}

impl KvStore {
    fn new() -> Self {
        KvStore { map: HashMap::new() }
    }
}