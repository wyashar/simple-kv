use std::hash::{BuildHasher, Hash, RandomState};

const START_CAPACITY: usize = 16;
const RESIZE_FACTOR: f32 = 0.75;

pub struct KeyStore<K, V, S = RandomState> {
    len: usize,
    buckets: Box<[Option<Entry<K, V>>]>,
    hasher: S,
}

pub struct Entry<K, V> {
    key: K,
    value: V,
    psl: usize,
    hash: u64,
}

impl<K, V> Entry<K, V> {
    pub fn new(key: K, value: V, hash: u64) -> Self {
        Self {
            key,
            value,
            psl: 0,
            hash,
        }
    }
}

impl<K, V> KeyStore<K, V> {
    pub fn new() -> Self {
        Self::with_hasher(RandomState::new())
    }
}

impl<K, V, S> KeyStore<K, V, S> {
    pub fn with_hasher(hasher: S) -> Self {
        Self {
            len: 0,
            buckets: Box::new([const { None}; START_CAPACITY]),
            hasher,
        }
    }

    fn should_resize(&self) -> bool {
        self.len as f32 / self.buckets.len() as f32 >= RESIZE_FACTOR
    }

    fn resize(&mut self) {
        let new_capacity = self.buckets.len() * 2;
        let new_buckets = (0..new_capacity).map(|_| None).collect();
        let old_buckets = std::mem::replace(&mut self.buckets, new_buckets);

        for entry in old_buckets {
            if let Some(e) = entry {
                self.insert_entry(e);
            }
        }
    }

    fn insert_entry(&mut self, mut entry: Entry<K, V>) {
        entry.psl = 0;
        loop {
            let index = self.bucket_index(entry.hash, entry.psl);
            match &mut self.buckets[index] {
                None => {
                    self.buckets[index] = Some(entry);
                    return;
                }
                Some(e) => {
                    if e.psl < entry.psl {
                        std::mem::swap(e, &mut entry);
                    }
                }
            }
            entry.psl += 1;
        }
    }

    fn bucket_index(&self, hash: u64, psl: usize) -> usize {
        (hash as usize + psl) & (self.buckets.len() - 1)
    }

    pub fn get(&self, key: &K) -> Option<&V>
    where
        K: Eq + Hash,
        S: BuildHasher,
    {
        let hash = self.hasher.hash_one(key);
        let mut psl: usize = 0;
        loop {
            let index = self.bucket_index(hash, psl);
            match &self.buckets[index] {
                None => {
                    return None;
                }
                Some(e) => {
                    if e.psl < psl {
                        return None;
                    }

                    if e.hash == hash && e.key == *key {
                        return Some(&e.value);
                    }
                }
            }

            psl += 1;
        }
    }

    pub fn insert(&mut self, key: K, value: V)
    where
        K: Eq + Hash,
        S: BuildHasher,
    {
        let hash = self.hasher.hash_one(&key);
        let mut entry = Entry::new(key, value, hash);
        let mut index = self.bucket_index(entry.hash, entry.psl);

        loop {
            match &mut self.buckets[index] {
                None => {
                    self.buckets[index] = Some(entry);
                    self.len += 1;
                    if self.should_resize() {
                        self.resize();
                    }
                    return;
                }
                Some(e) => {
                    if e.hash == entry.hash && e.key == entry.key {
                        e.value = entry.value;
                        return;
                    }
                    if e.psl < entry.psl {
                        std::mem::swap(e, &mut entry);
                    }
                }
            }
            entry.psl += 1;
            index = self.bucket_index(entry.hash, entry.psl);
        }
    }
}
