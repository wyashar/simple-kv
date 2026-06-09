use std::fmt;
use std::hash::{BuildHasher, Hash, Hasher};

use rustc_hash::FxBuildHasher;

const STARTING_CAPACITY: usize = 16;
const LOAD_FACTOR: f32 = 0.75;

pub struct KvStore<K, V> {
    len: usize,
    buckets: Box<[Option<KvEntry<K, V>>]>,
    hasher: FxBuildHasher,
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
            buckets: Box::new([const { None }; STARTING_CAPACITY]),
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
        (self.len as f32 / self.buckets.len() as f32) >= LOAD_FACTOR
    }

    fn resize(&mut self) {
        let new_buckets: Box<[Option<KvEntry<K, V>>]> =
            (0..self.buckets.len() * 2).map(|_| None).collect();
        let old_buckets: Box<[Option<KvEntry<K, V>>]> =
            std::mem::replace(&mut self.buckets, new_buckets);

        for bucket in old_buckets {
            if let Some(entry) = bucket {
                self.insert_entry(entry);
            }
        }
    }

    fn insert_entry(&mut self, mut entry: KvEntry<K, V>) {
        entry.psl = 0;

        loop {
            let bucket_index: usize = self.get_bucket_index(entry.hash, entry.psl);
            match &mut self.buckets[bucket_index] {
                Some(e) if e.psl < entry.psl => {
                    std::mem::swap(e, &mut entry);
                }
                None => {
                    self.buckets[bucket_index] = Some(entry);
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
                Some(e) => {
                    if e.psl < psl {
                        return None;
                    }
                    // backward shift deletion
                    if e.hash == hash && e.key == *key {
                        self.len -= 1;

                        let hole = bucket_index;
                        let deleted_value = self.buckets[hole]
                            .take()
                            .expect("located bucket was occupied")
                            .value;

                        // backward-shift: pull the following run one slot toward home,
                        // stopping at the first empty or psl-0 entry. `& mask` wraps the
                        // probe around the ring.
                        let mask = self.buckets.len() - 1;
                        let mut idx = hole;
                        loop {
                            let next = (idx + 1) & mask;
                            match &mut self.buckets[next] {
                                Some(entry) if entry.psl > 0 => entry.psl -= 1,
                                _ => break,
                            }
                            self.buckets.swap(idx, next);
                            idx = next;
                        }

                        return Some(deleted_value);
                    }
                }
                None => return None,
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
                Some(e) => {
                    if e.hash == hash && e.key == *key {
                        return Some(&e.value);
                    }
                    if e.psl < psl {
                        return None;
                    }
                }
                None => return None,
            }

            psl += 1;
        }
    }

    pub fn put(&mut self, key: K, value: V) {
        let hash: usize = self.hash_key(&key);
        let mut incoming: KvEntry<K, V> = KvEntry::new(key, value, hash, 0);

        if self.should_resize() {
            self.resize();
        }

        loop {
            let bucket_index: usize = self.get_bucket_index(incoming.hash, incoming.psl);

            match &mut self.buckets[bucket_index] {
                Some(e) => {
                    // key collision, rewrite
                    if e.hash == incoming.hash && e.key == incoming.key {
                        e.value = incoming.value;
                        return;
                    }
                    // robin hood swap
                    if e.psl < incoming.psl {
                        std::mem::swap(e, &mut incoming)
                    }
                }
                // if you see an empty, that means you already probed past possible key collison idxs
                None => {
                    self.len += 1;
                    self.buckets[bucket_index] = Some(incoming);
                    return;
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
                Some(e) => format!("{{{}: {}, p{}}}", e.key, e.value, e.psl),
                None => String::from("_"),
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
        assert!(kv.len == 1);
        kv.del(&key);
        assert!(kv.len == 0);
        assert_eq!(kv.get(&key), None);
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
    fn robin_hood_swaps() {
        let mut kv: KvStore<String, i32> = KvStore::new();

        for i in 1..=10 {
            kv.put(format!("key{i}"), i);
        }

        // [_, {key4: 4, p0}, _, _, {key7: 7, p0}, _, _, {key2: 2, p0} (key 58 hashes here), {key5: 5, p1}, {key8: 8, p2}, {key9: 9, p2}, {key1: 1, p0}, {key6: 6, p1}, {key10: 10, p2}, {key3: 3, p2}, _]
        // classic robin hood swap example
        // key58 hashes to bucket index of 7, it will probe until it hits entry.psl < incoming.psl (key9), then key9 swaps with key1, which swaps with key3, which gets written to the next empty slot
        kv.put(String::from("key58"), 58);
        assert!(matches!(&kv.buckets[10], Some(e) if e.key == "key58" && e.value == 58));
        assert!(matches!(&kv.buckets[11], Some(e) if e.key == "key9" && e.psl == 3));
        assert!(matches!(&kv.buckets[14], Some(e) if e.key == "key1" && e.psl == 3));
        assert!(matches!(&kv.buckets[15], Some(e) if e.key == "key3" && e.psl == 3));
        /*
        * [_, {key4: 4, p0}, _, _, {key7: 7, p0}, _, _, {key2: 2, p0}, {key5: 5, p1}, {key8: 8, p2}, {key9: 9, p2}, {key1: 1, p0}, {key6: 6, p1}, {key10: 10, p2}, {key3: 3, p2}, _]
           [_, {key4: 4, p0}, _, _, {key7: 7, p0}, _, _, {key2: 2, p0}, {key5: 5, p1}, {key8: 8, p2}, {key58: 58, p3}, {key9: 9, p3}, {key6: 6, p1}, {key10: 10, p2}, {key1: 1, p3}, {key3: 3, p3}]
        */
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
        assert!(matches!(&kv.buckets[14], Some(e) if e.key == "key1"));

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
        assert!(matches!(&kv.buckets[14], Some(e) if e.key == "key1"));

        assert_eq!(kv.del(&String::from("key1")), Some(1));
        assert_eq!(kv.len(), 10);
        assert_eq!(kv.get(&String::from("key1")), None);
        // backward-shift pulls key3 from 15 back into 14 (psl 3 -> 2), leaving 15 Empty
        assert!(matches!(&kv.buckets[14], Some(e) if e.key == "key3" && e.psl == 2));
        assert!(matches!(&kv.buckets[15], None));
    }

    #[test]
    fn del_shifts_across_ring_boundary() {
        let mut kv: KvStore<i32, i32> = KvStore::new();
        let mask = kv.buckets.len() - 1;

        // 26, 37, 48 all hash to the last slot (mask=15), so their cluster wraps to 0, 1
        let (a, b, c) = (26, 37, 48);
        kv.put(a, 1); // -> slot 15 (psl 0)
        kv.put(b, 2); // -> slot 0  (psl 1, wrapped past the boundary)
        kv.put(c, 3); // -> slot 1  (psl 2, wrapped)
        assert!(matches!(&kv.buckets[mask], Some(e) if e.key == a && e.psl == 0));
        assert!(matches!(&kv.buckets[0], Some(e) if e.key == b && e.psl == 1));
        assert!(matches!(&kv.buckets[1], Some(e) if e.key == c && e.psl == 2));

        // delete the head at the last slot; b and c must shift back ACROSS the boundary
        assert_eq!(kv.del(&a), Some(1));
        assert!(matches!(&kv.buckets[mask], Some(e) if e.key == b && e.psl == 0));
        assert!(matches!(&kv.buckets[0], Some(e) if e.key == c && e.psl == 1));
        assert!(matches!(&kv.buckets[1], None));

        // survivors stay findable, the deleted key is gone
        assert_eq!(kv.get(&a), None);
        assert_eq!(kv.get(&b), Some(&2));
        assert_eq!(kv.get(&c), Some(&3));
    }

    #[test]
    fn del_keeps_survivors_findable_and_psl_consistent() {
        let mut kv: KvStore<i32, i32> = KvStore::new();

        // enough entries to force several resizes and lots of clustering
        for i in 0..400 {
            kv.put(i, i * 10);
        }
        // delete every even key
        for i in (0..400).step_by(2) {
            assert_eq!(kv.del(&i), Some(i * 10));
        }
        assert_eq!(kv.len(), 200);

        // odds survive with their values, evens are gone
        for i in 0..400 {
            if i % 2 == 0 {
                assert_eq!(kv.get(&i), None, "even key {i} should be deleted");
            } else {
                assert_eq!(kv.get(&i), Some(&(i * 10)), "odd key {i} should survive");
            }
        }

        // every occupied entry's stored psl must equal its actual displacement from home --
        // the invariant backward-shift is responsible for maintaining
        let mask = kv.buckets.len() - 1;
        for (idx, bucket) in kv.buckets.iter().enumerate() {
            if let Some(e) = bucket {
                assert_eq!(
                    e.psl,
                    idx.wrapping_sub(e.hash) & mask,
                    "psl mismatch at index {idx}"
                );
            }
        }
    }
}
