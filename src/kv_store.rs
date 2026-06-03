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

        let mut probe_dist: usize = 0;
        loop {
            let bucket_index: usize = self.get_bucket_index(hash, probe_dist);
            let bucket: &Bucket<K, V> = &self.buckets[bucket_index];

            match bucket {
                Empty => return None,
                Occupied(entry) => {
                    if entry.hash == hash && entry.key == *key {
                        self.len -= 1;
                        self.tombstones_count += 1;
                        let old =
                            std::mem::replace(&mut self.buckets[bucket_index], Bucket::Tombstone);

                        if let Occupied(e) = old {
                            return Some(e.value);
                        } else {
                            unreachable!(
                                "It is guarenteed for this partial function to be a full function!"
                            );
                        }
                    }
                }
                Tombstone => {}
            }

            probe_dist = probe_dist + 1;
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let hash: usize = self.hash_key(key);

        let mut probe_dist: usize = 0;
        loop {
            let bucket_index: usize = self.get_bucket_index(hash, probe_dist);
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

            probe_dist = probe_dist + 1;
        }
    }

    pub fn put(&mut self, key: K, value: V) -> () {
        let hash: usize = self.hash_key(&key);

        let mut probe_dist: usize = 0;
        let mut tombstone_index: Option<usize> = None;
        loop {
            let bucket_index: usize = self.get_bucket_index(hash, probe_dist);
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
                    self.buckets[write_index] = Bucket::Occupied(KvEntry::new(key, value, hash));
                    return;
                }
                Occupied(entry) => {
                    if entry.hash == hash && entry.key == key {
                        if let Some(t_index) = tombstone_index {
                            self.buckets[t_index] =
                                Bucket::Occupied(KvEntry::new(key, value, hash));
                            self.buckets[bucket_index] = Bucket::Tombstone;
                        } else {
                            entry.value = value;
                        }
                        return;
                    }
                }
            }

            probe_dist = probe_dist + 1;
        }
    }
}
