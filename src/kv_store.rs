use std::hash::{BuildHasher, Hash, Hasher};

use rustc_hash::FxBuildHasher;

use crate::kv_store::Bucket::{Empty, Occupied, Tombstone};

const STARTING_CAPACITY: usize = 16;
const LOAD_FACTOR: f32 = 0.75;

pub struct KvStore<K, V> {
    size: usize,
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
}

impl<K, V> KvEntry<K, V> {
    fn new(key: K, value: V, hash: usize) -> Self {
        Self { key, value, hash }
    }
}

impl<K: Hash + Eq, V> KvStore<K, V> {
    fn get_bucket_index(&self, hash: usize, probe_dist: usize) -> usize {
        (hash + probe_dist) & (self.buckets.len() - 1)
    }

    fn hash_key(&self, key: &K) -> usize {
        let mut hasher = self.hasher.build_hasher();
        key.hash(&mut hasher);
        hasher.finish() as usize
    }

    pub fn new() -> Self {
        Self {
            size: 0,
            buckets: Box::new([const { Bucket::Empty }; STARTING_CAPACITY]),
            hasher: FxBuildHasher::default(),
        }
    }

    pub fn put(&mut self, key: K, value: V) -> () {
        let hash: usize = self.hash_key(&key);

        let mut probe_dist: usize = 0;
        loop {
            let bucket_index: usize = self.get_bucket_index(hash, probe_dist);
            let bucket: &mut Bucket<K, V> = &mut self.buckets[bucket_index];

            // TODO: handle the tombstone case
            match bucket {
                Empty => {
                    self.buckets[bucket_index] = Bucket::Occupied(KvEntry::new(key, value, hash));
                    return;
                }
                Occupied(entry) => {
                    if entry.hash == hash && entry.key == key {
                        entry.value = value;
                        return;
                    }
                }
            }

            probe_dist = probe_dist + 1;
        }
    }
}
