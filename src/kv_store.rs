use std::hash::{BuildHasher, Hash, Hasher};

use rustc_hash::FxBuildHasher;

use crate::kv_store::Bucket::{Empty, Occupied, Tombstone};

const STARTING_CAPACITY: usize = 16;
const LOAD_FACTOR: f32 = 0.75;

pub struct KvStore<K, V> {
    len: usize,
    tombstones_count: usize,
    buckets: Box<[Bucket<K, V>]>,
    hasher: FxBuildHasher,
}

enum Bucket<K, V> {
    Empty,
    Tombstone,
    Occupied(KvEntry<K, V>),
}

struct KvEntry<K, V> {
    key: K,
    value: V,
    hash: usize,
    psl: usize,
}

impl<K, V> KvEntry<K, V> {
    fn new(key: K, value: V, hash: usize, psl: usize) -> Self {
        Self {
            key,
            value,
            hash,
            psl,
        }
    }
}

impl<K: Hash + Eq, V> KvStore<K, V> {
    fn get_bucket_index(&self, hash: usize, psl: usize) -> usize {
        (hash + psl) & (self.buckets.len() - 1)
    }

    fn hash_key(&self, key: &K) -> usize {
        let mut hasher = self.hasher.build_hasher();
        key.hash(&mut hasher);
        hasher.finish() as usize
    }

    pub fn new() -> Self {
        Self {
            len: 0,
            tombstones_count: 0,
            buckets: Box::new([const { Bucket::Empty }; STARTING_CAPACITY]),
            hasher: FxBuildHasher::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn del(&mut self, key: &K) -> Option<V> {
        let hash: usize = self.hash_key(key);

        let mut psl: usize = 0;
        loop {
            let bucket_index: usize = self.get_bucket_index(hash, psl);
            let bucket: &Bucket<K, V> = &self.buckets[bucket_index];

            match bucket {
                Empty => return None,
                Occupied(entry) => {
                    if entry.hash == hash && entry.key == *key {
                        self.len -= 1;
                        self.tombstones_count += 1;

                        match std::mem::replace(&mut self.buckets[bucket_index], Bucket::Tombstone)
                        {
                            Occupied(e) => return Some(e.value),
                            _ => unreachable!(
                                "Occupied(_) is guarenteed to act as a non-partial function here!"
                            ),
                        }
                    }
                }
                Tombstone => {}
            }

            psl = psl + 1;
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let hash: usize = self.hash_key(key);

        let mut psl: usize = 0;
        loop {
            let bucket_index: usize = self.get_bucket_index(hash, psl);
            let bucket: &Bucket<K, V> = &self.buckets[bucket_index];

            match bucket {
                Empty => return None,
                Occupied(entry) => {
                    if entry.hash == hash && entry.key == *key {
                        return Some(&entry.value);
                    }
                }
                Tombstone => {}
            }

            psl = psl + 1;
        }
    }

    pub fn put(&mut self, key: K, value: V) -> () {
        let hash: usize = self.hash_key(&key);

        let mut psl: usize = 0;
        let mut tombstone_index: Option<usize> = None;
        loop {
            let bucket_index: usize = self.get_bucket_index(hash, psl);
            let bucket: &mut Bucket<K, V> = &mut self.buckets[bucket_index];

            match bucket {
                Tombstone => {
                    if tombstone_index.is_none() {
                        tombstone_index = Some(bucket_index);
                    }
                }
                Empty => {
                    self.len += 1;
                    let write_index: usize = tombstone_index.unwrap_or(bucket_index);
                    if tombstone_index.is_some() {
                        self.tombstones_count -= 1;
                    }
                    self.buckets[write_index] =
                        Bucket::Occupied(KvEntry::new(key, value, hash, psl));
                    return;
                }
                Occupied(entry) => {
                    if entry.hash == hash && entry.key == key {
                        match tombstone_index {
                            Some(t_index) => {
                                self.buckets[t_index] =
                                    Bucket::Occupied(KvEntry::new(key, value, hash, psl));
                                self.buckets[bucket_index] = Bucket::Tombstone;
                            }
                            None => entry.value = value,
                        }
                        return;
                    }
                }
            }

            psl = psl + 1;
        }
    }
}
