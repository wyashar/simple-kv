use std::fmt;
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

    fn fill_tombstone(&mut self, t_idx: usize, t_psl: usize, mut incoming: KvEntry<K, V>) {
        self.len += 1;
        self.tombstones_count -= 1;
        incoming.psl = t_psl;
        self.buckets[t_idx] = Occupied(incoming);
    }

    pub fn del(&mut self, key: &K) -> Option<V> {
        let hash: usize = self.hash_key(key);

        let mut psl: usize = 0;
        loop {
            let bucket_index: usize = self.get_bucket_index(hash, psl);

            match &self.buckets[bucket_index] {
                Occupied(e) => {
                    if e.psl < psl {
                        return None;
                    }
                    if e.hash == hash && e.key == *key {
                        self.len -= 1;
                        self.tombstones_count += 1;
                        let tombstoned_bucket: Bucket<K, V> =
                            std::mem::replace(&mut self.buckets[bucket_index], Tombstone);

                        match tombstoned_bucket {
                            Occupied(entry) => return Some(entry.value),
                            _ => unreachable!("Occupied(_) is a non-partial function here"),
                        }
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
                Occupied(e) => {
                    if e.hash == hash && e.key == *key {
                        return Some(&e.value);
                    }
                    if e.psl < psl {
                        return None;
                    }
                }
                Empty => return None,
                _ => {}
            }

            psl += 1;
        }
    }

    pub fn put(&mut self, key: K, value: V) {
        let hash: usize = self.hash_key(&key);
        let mut incoming: KvEntry<K, V> = KvEntry::new(key, value, hash, 0);
        let mut tombstone_slot: Option<(usize, usize)> = None; // (index, psl)

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
                        match tombstone_slot {
                            // cancel the robin hood swap and fill the earlier hole instead
                            Some((t_idx, t_psl)) => {
                                self.fill_tombstone(t_idx, t_psl, incoming);
                                return;
                            }
                            None => std::mem::swap(e, &mut incoming),
                        }
                    }
                }
                // if you see an empty, that means you already probed past possible key collison idxs
                Empty => {
                    match tombstone_slot {
                        // consume tombstone if seen
                        Some((t_idx, t_psl)) => self.fill_tombstone(t_idx, t_psl, incoming),
                        None => {
                            self.len += 1;
                            self.buckets[bucket_index] = Occupied(incoming);
                        }
                    }
                    return;
                }
                // just because we see a tombstone doesn't mean we can swap right away since we may have a key collision later on
                Tombstone => {
                    if tombstone_slot.is_none() {
                        tombstone_slot = Some((bucket_index, incoming.psl));
                    }
                }
            }

            incoming.psl += 1;
        }
    }
}

impl<K: fmt::Display, V: fmt::Display> fmt::Display for KvStore<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries: Vec<String> = self
            .buckets
            .iter()
            .map(|b| match b {
                Occupied(e) => format!("{{{}: {}, p{}}}", e.key, e.value, e.psl),
                Tombstone => String::from("Tombstone"),
                Empty => String::from("_"),
            })
            .collect();
        write!(f, "[{}]", entries.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_put() {
        let mut kv: KvStore<i32, String> = KvStore::new();
        let key: i32 = 102;
        let value: String = String::from("value");

        kv.put(key, value.clone());
        assert_eq!(kv.get(&key), Some(&value));
        assert!(kv.tombstones_count == 0);
        assert!(kv.len == 1);
        kv.del(&key);
        assert!(kv.len == 0);
        assert!(kv.tombstones_count == 1);
    }

    #[test]
    fn key_collisons_evict() {
        let mut kv: KvStore<String, i32> = KvStore::new();

        for i in 1..=6 {
            kv.put(format!("key{i}"), i);
        }

        assert!(kv.len == 6);
        kv.put(String::from("key5"), 999);
        assert_eq!(kv.get(&String::from("key5")), Some(&(999 as i32)));
        assert!(kv.len == 6);
    }

    #[test]
    fn tombstone_fill_when_empty_insertion() {
        let mut kv: KvStore<String, i32> = KvStore::new();

        for i in 1..=6 {
            kv.put(format!("key{i}"), i);
        }

        // [T, 4, T, _, _, _, _, 2, 5, _, _, 1, 6, 3, _, _]
        kv.del(&String::from("key5"));
        // [_, 4, _, _, _, _, _, 2, Tombstone, _, _, 1, 6, 3, _, _]

        // key8 will hash to where key2 is, then traverse Tombstone, _, it should insert at Tombstone, instead of Tombstone + 1
        kv.put(String::from("key8"), 8);
        assert!(matches!(&kv.buckets[8], Occupied(e) if e.key == "key8" && e.value == 8));
        // [_, 4, _, _, _, _, _, 2, 8, _, _, 1, 6, 3, _, _]

        // now the case where we have multiple tombstones in a row and we only consider the first one
        kv.put(String::from("key40"), 40);
        kv.put(String::from("key17"), 17);

        kv.del(&String::from("key40"));
        kv.del(&String::from("key17"));
        kv.del(&String::from("key1"));

        // [_, 4, _, _, _, _, _, 2, 8, Tombstone, Tombstone, Tombstone, 6, 3, _, _]
        // key60 hashes to bucket 8
        kv.put(String::from("key60"), 60);
        assert!(matches!(&kv.buckets[9], Occupied(e) if e.key == "key60" && e.value == 60));
        // [_, 4, _, _, _, _, _, 2, 8, 60, Tombstone, Tombstone, 6, 3, _, _]
    }

    #[test]
    fn robin_hood_swaps() {
        let mut kv: KvStore<String, i32> = KvStore::new();

        for i in 1..=10 {
            kv.put(format!("key{i}"), i);
        }

        // [_, {key4: 4, p0}, _, _, {key7: 7, p0}, _, _, {key2: 2, p0} (key 58 hashes here), {key5: 5, p1}, {key8: 8, p2}, {key9: 9, p2}, {key1: 1, p0}, {key6: 6, p1}, {key10: 10, p2}, {key3: 3, p2}, _]
        // classic robin hood swap example
        // key58 hashes to bucket index of 7, it will probe until it hits entry.psl < incoming.psl (key9), then key9 swaps with key1, which swaps with key3, which gets written to the next empty slot
        kv.put(String::from("key58"), 58);
        assert!(matches!(&kv.buckets[10], Occupied(e) if e.key == "key58" && e.value == 58));
        assert!(matches!(&kv.buckets[11], Occupied(e) if e.key == "key9" && e.psl == 3));
        assert!(matches!(&kv.buckets[14], Occupied(e) if e.key == "key1" && e.psl == 3));
        assert!(matches!(&kv.buckets[15], Occupied(e) if e.key == "key3" && e.psl == 3));
        /*
        * [_, {key4: 4, p0}, _, _, {key7: 7, p0}, _, _, {key2: 2, p0}, {key5: 5, p1}, {key8: 8, p2}, {key9: 9, p2}, {key1: 1, p0}, {key6: 6, p1}, {key10: 10, p2}, {key3: 3, p2}, _]
           [_, {key4: 4, p0}, _, _, {key7: 7, p0}, _, _, {key2: 2, p0}, {key5: 5, p1}, {key8: 8, p2}, {key58: 58, p3}, {key9: 9, p3}, {key6: 6, p1}, {key10: 10, p2}, {key1: 1, p3}, {key3: 3, p3}]
        */
        // now we make sure we cancel the robinhood swap if we encounter a tombstone
        // key103 hashes to bucket 7
        // bucket 9 is Tombstoned (key8 lived there at p2), so key103 fills it instead of displacing key58
        kv.del(&String::from("key8"));
        assert_eq!(kv.tombstones_count, 1);
        kv.put(String::from("key103"), 103);
        assert!(matches!(&kv.buckets[9], Occupied(e) if e.key == "key103" && e.value == 103));
        assert!(matches!(&kv.buckets[14], Occupied(e) if e.key == "key1" && e.value == 1));
        assert_eq!(kv.tombstones_count, 0);
    }

    #[test]
    fn resize_when_at_load_factor() {
        let mut kv: KvStore<String, i32> = KvStore::new();
        let initial_capacity = kv.buckets.len();
        let trigger = (initial_capacity as f32 * LOAD_FACTOR) as usize;

        // should_resize fires pre-insert, so inserting exactly `trigger` entries doesn't resize yet
        for i in 0..trigger {
            kv.put(format!("key{i}"), i as i32);
        }
        assert_eq!(kv.buckets.len(), initial_capacity);

        // one more tips (len / capacity) to >= LOAD_FACTOR, resize fires before this insert
        kv.put(String::from("extra"), 0);
        assert_eq!(kv.buckets.len(), initial_capacity * 2);
        assert_eq!(kv.tombstones_count, 0);
        assert!(kv.buckets.iter().all(|b| !matches!(b, Tombstone)));

        for i in 0..trigger {
            assert_eq!(kv.get(&format!("key{i}")), Some(&(i as i32)));
        }
    }

    #[test]
    fn update_displaced_key() {
        let mut kv: KvStore<String, i32> = KvStore::new();

        for i in 1..=10 {
            kv.put(format!("key{i}"), i);
        }
        kv.put(String::from("key58"), 58);

        // key1 was displaced to index 14 by the robin hood swap chain
        assert!(matches!(&kv.buckets[14], Occupied(e) if e.key == "key1"));

        kv.put(String::from("key1"), 999);

        assert_eq!(kv.get(&String::from("key1")), Some(&999));
        assert_eq!(kv.len(), 11);
    }

    #[test]
    fn get_psl_short_circuit() {
        let mut kv: KvStore<String, i32> = KvStore::new();

        for i in 1..=10 {
            kv.put(format!("key{i}"), i);
        }
        kv.put(String::from("key58"), 58);

        // key103 hashes to index 7 (same cluster as key2/key5/key8/key58)
        // probing reaches key9 at index 11 with psl=3, but our probe distance is 4
        // key103 would have displaced key9 if it existed, so we short-circuit here instead of scanning to Empty
        assert_eq!(kv.get(&String::from("key103")), None);
    }

    #[test]
    fn del_psl_short_circuit() {
        let mut kv: KvStore<String, i32> = KvStore::new();

        for i in 1..=10 {
            kv.put(format!("key{i}"), i);
        }
        kv.put(String::from("key58"), 58);

        // same short-circuit as get — del returns None without scanning to Empty
        assert_eq!(kv.del(&String::from("key103")), None);
        assert_eq!(kv.len(), 11);
    }

    #[test]
    fn del_displaced_key() {
        let mut kv: KvStore<String, i32> = KvStore::new();

        for i in 1..=10 {
            kv.put(format!("key{i}"), i);
        }
        kv.put(String::from("key58"), 58);

        // key1 was displaced to index 14 by the robin hood swap chain
        assert!(matches!(&kv.buckets[14], Occupied(e) if e.key == "key1"));

        assert_eq!(kv.del(&String::from("key1")), Some(1));
        assert_eq!(kv.len(), 10);
        assert_eq!(kv.get(&String::from("key1")), None);
        assert!(matches!(&kv.buckets[14], Tombstone));
    }

    #[test]
    fn get_and_del_probe_through_tombstones() {
        let mut kv: KvStore<String, i32> = KvStore::new();

        for i in 1..=6 {
            kv.put(format!("key{i}"), i);
        }

        // key2 at index 7 (p0) and key5 at index 8 (p1) both hash to 7
        // deleting key2 puts a tombstone at index 7
        kv.del(&String::from("key2"));
        assert!(matches!(&kv.buckets[7], Tombstone));

        // key5 is still reachable — get/del must probe through the tombstone at 7
        assert_eq!(kv.get(&String::from("key5")), Some(&5));
        assert_eq!(kv.del(&String::from("key5")), Some(5));
        assert_eq!(kv.get(&String::from("key5")), None);
    }
}
