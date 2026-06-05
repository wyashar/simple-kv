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

impl<K: Eq, V> KvEntry<K, V> {
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
        hash.wrapping_add(psl) & (self.buckets.len() - 1)
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

    fn should_resize(&self) -> bool {
        ((self.len + self.tombstones_count) as f32 / self.buckets.len() as f32) >= LOAD_FACTOR
    }

    fn resize(&mut self) {
        self.tombstones_count = 0;
        let new_buckets: Box<[Bucket<K, V>]> =
            (0..self.buckets.len() * 2).map(|_| Bucket::Empty).collect();
        let old_buckets: Box<[Bucket<K, V>]> = std::mem::replace(&mut self.buckets, new_buckets);

        for bucket in old_buckets {
            if let Occupied(entry) = bucket {
                self.insert_entry(entry);
            }
        }
    }

    fn insert_entry(&mut self, mut entry: KvEntry<K, V>) {
        entry.psl = 0;

        loop {
            let bucket_index: usize = self.get_bucket_index(entry.hash, entry.psl);
            match &mut self.buckets[bucket_index] {
                Occupied(e) if e.psl < entry.psl => {
                    std::mem::swap(e, &mut entry);
                }
                Empty => {
                    self.buckets[bucket_index] = Occupied(entry);
                    return;
                }
                _ => {}
            }

            entry.psl += 1;
        }
    }

    pub fn del(&mut self, key: &K) -> Option<V> {
        let hash: usize = self.hash_key(key);

        let mut psl: usize = 0;
        loop {
            let bucket_index: usize = self.get_bucket_index(hash, psl);

            match &self.buckets[bucket_index] {
                Occupied(e) if e.psl < psl => return None,
                Occupied(e) if e.hash == hash && e.key == *key => {
                    self.len -= 1;
                    self.tombstones_count += 1;
                    let tombstoned_bucket: Bucket<K, V> =
                        std::mem::replace(&mut self.buckets[bucket_index], Bucket::Tombstone);

                    match tombstoned_bucket {
                        Occupied(entry) => return Some(entry.value),
                        _ => unreachable!("Occupied(_) is a non-partial function here"),
                    }
                }
                Empty => return None,
                _ => {}
            }

            psl += 1;
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let hash: usize = self.hash_key(key);
        let mut psl = 0;

        loop {
            let bucket_index: usize = self.get_bucket_index(hash, psl);

            match &self.buckets[bucket_index] {
                Occupied(e) if e.hash == hash && e.key == *key => return Some(&e.value),
                Occupied(e) if e.psl < psl => return None,
                Empty => return None,
                _ => {}
            }

            psl += 1;
        }
    }

    pub fn put(&mut self, key: K, value: V) {
        let hash: usize = self.hash_key(&key);
        let mut incoming: KvEntry<K, V> = KvEntry::new(key, value, hash, 0);
        let mut tombstone_idxs: Option<(usize, usize)> = None; // (index, psl)

        if self.should_resize() {
            self.resize();
        }

        loop {
            let bucket_index: usize = self.get_bucket_index(incoming.hash, incoming.psl);

            match &mut self.buckets[bucket_index] {
                Occupied(e) => {
                    // key collision, rewrite
                    if e.hash == incoming.hash && e.key == incoming.key {
                        e.value = incoming.value;
                        return;
                    }
                    // robin hood swap
                    if e.psl < incoming.psl {
                        match tombstone_idxs {
                            // cancel the robin hood swap and fill the earlier hole instead
                            Some((t_idx, t_psl)) => {
                                self.tombstones_count -= 1;
                                self.len += 1;
                                incoming.psl = t_psl;
                                self.buckets[t_idx] = Bucket::Occupied(incoming);
                                return;
                            }
                            None => std::mem::swap(e, &mut incoming),
                        }
                    }
                }
                // if you see an empty, that means you already probed past possible key collison idxs
                Empty => {
                    self.len += 1;
                    match tombstone_idxs {
                        // consume tombstone if seen
                        Some((t_idx, t_psl)) => {
                            self.tombstones_count -= 1;
                            incoming.psl = t_psl;
                            self.buckets[t_idx] = Bucket::Occupied(incoming);
                        }
                        None => {
                            self.buckets[bucket_index] = Bucket::Occupied(incoming);
                        }
                    }
                    return;
                }
                // just because we see a tombstone doesn't mean we can swap right away since we may have a key collision later on
                Tombstone => {
                    if tombstone_idxs.is_none() {
                        tombstone_idxs = Some((bucket_index, incoming.psl));
                    }
                }
            }

            incoming.psl += 1;
        }
    }
}
