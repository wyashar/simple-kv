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
                    if entry.psl < psl {
                        return None;
                    }

                    if entry.hash == hash && entry.key == *key {
                        self.len -= 1;
                        self.tombstones_count += 1;

                        match std::mem::replace(&mut self.buckets[bucket_index], Bucket::Tombstone)
                        {
                            Occupied(e) => return Some(e.value),
                            _ => unreachable!(
                                "Occupied(_) is guaranteed to act as a non-partial function here!"
                            ),
                        }
                    }
                }
                Tombstone => {}
            }

            psl += 1;
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
                    if entry.psl < psl {
                        return None;
                    }

                    if entry.hash == hash && entry.key == *key {
                        return Some(&entry.value);
                    }
                }
                Tombstone => {}
            }

            psl += 1;
        }
    }

    pub fn put(&mut self, key: K, value: V) {
        let mut key = key;
        let mut value = value;
        let mut hash: usize = self.hash_key(&key);

        let mut psl: usize = 0;
        let mut tombstone_idxs: Option<(usize, usize)> = None; // (tombstone_index, tombstone_psl)
        loop {
            let bucket_index: usize = self.get_bucket_index(hash, psl);
            let bucket: &mut Bucket<K, V> = &mut self.buckets[bucket_index];
            match bucket {
                Tombstone => {
                    if tombstone_idxs.is_none() {
                        tombstone_idxs = Some((bucket_index, psl));
                    }
                }
                Empty => {
                    self.len += 1;
                    match tombstone_idxs {
                        Some((t_idx, t_psl)) => {
                            self.tombstones_count -= 1;
                            self.buckets[t_idx] =
                                Bucket::Occupied(KvEntry::new(key, value, hash, t_psl));
                        }
                        None => {
                            *bucket = Bucket::Occupied(KvEntry::new(key, value, hash, psl));
                        }
                    }
                    return;
                }
                Occupied(entry) => {
                    if entry.hash == hash && entry.key == key {
                        match tombstone_idxs {
                            Some((t_index, t_psl)) => {
                                self.buckets[t_index] =
                                    Bucket::Occupied(KvEntry::new(key, value, hash, t_psl));
                                self.buckets[bucket_index] = Bucket::Tombstone;
                            }
                            None => entry.value = value,
                        }
                        return;
                    }

                    if entry.psl < psl {
                        if let Some((t_idx, t_psl)) = tombstone_idxs {
                            self.len += 1;
                            self.tombstones_count -= 1;
                            self.buckets[t_idx] =
                                Bucket::Occupied(KvEntry::new(key, value, hash, t_psl));
                            return;
                        }

                        let stolen_bucket: Bucket<K, V> = std::mem::replace(
                            bucket,
                            Bucket::Occupied(KvEntry::new(key, value, hash, psl)),
                        );

                        match stolen_bucket {
                            Occupied(e) => {
                                key = e.key;
                                value = e.value;
                                hash = e.hash;
                                psl = e.psl;
                            }
                            _ => unreachable!(
                                "Occupied(_) is guaranteed to act as a non-partial function here!"
                            ),
                        }
                    }
                }
            }

            psl += 1;
        }
    }
}
