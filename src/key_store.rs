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

impl<K, V> Default for KeyStore<K, V> {
    fn default() -> Self {
        Self::with_hasher(RandomState::new())
    }
}

impl<K, V, S> KeyStore<K, V, S> {
    pub fn with_hasher(hasher: S) -> Self {
        Self {
            len: 0,
            buckets: Box::new([const { None }; START_CAPACITY]),
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

        for entry in old_buckets.into_iter().flatten() {
            self.insert_entry(entry);
        }
    }

    fn insert_entry(&mut self, mut entry: Entry<K, V>) {
        entry.psl = 0;
        loop {
            let index = self.get_bucket_index(entry.hash, entry.psl);
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

    fn get_bucket_index(&self, hash: u64, psl: usize) -> usize {
        (hash as usize + psl) & (self.buckets.len() - 1)
    }

    pub fn del(&mut self, key: &K) -> Option<V>
    where
        K: Eq + Hash,
        S: BuildHasher,
    {
        let hash = self.hasher.hash_one(key);
        let mut psl = 0;
        loop {
            let index = self.get_bucket_index(hash, psl);
            match &self.buckets[index] {
                None => {
                    return None;
                }
                Some(e) => {
                    if e.psl < psl { // this invariant is maintained by insertion is robin hood swap
                        return None;
                    }

                    if e.hash == hash && e.key == *key {
                        let removed_entry = std::mem::take(&mut self.buckets[index])
                            .expect("present since inside some variant");
                        self.len -= 1;

                        let mask = self.buckets.len() - 1;
                        let mut idx = index;
                        loop {
                            let next = (idx + 1) & mask;
                            match &mut self.buckets[next] {
                                Some(e) if e.psl > 0 => e.psl -= 1,
                                _ => return Some(removed_entry.value),
                            }

                            self.buckets.swap(idx, next);
                            idx = next;
                        }
                    }
                }
            }

            psl += 1;
        }
    }

    pub fn get(&self, key: &K) -> Option<&V>
    where
        K: Eq + Hash,
        S: BuildHasher,
    {
        let hash = self.hasher.hash_one(key);
        let mut psl: usize = 0;
        loop {
            let index = self.get_bucket_index(hash, psl);
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

    pub fn insert(&mut self, key: K, value: V) -> Option<V>
    where
        K: Eq + Hash,
        S: BuildHasher,
    {
        let hash = self.hasher.hash_one(&key);
        let mut entry = Entry::new(key, value, hash);
        let mut index = self.get_bucket_index(entry.hash, entry.psl);

        loop {
            match &mut self.buckets[index] {
                None => {
                    self.buckets[index] = Some(entry);
                    self.len += 1;
                    if self.should_resize() {
                        self.resize();
                    }
                    return None;
                }
                Some(e) => {
                    if e.hash == entry.hash && e.key == entry.key {
                        return Some(std::mem::replace(&mut e.value, entry.value));
                    }
                    if e.psl < entry.psl { // robin hood swapping, richer entries reinserted
                        std::mem::swap(e, &mut entry);
                    }
                }
            }
            entry.psl += 1;
            index = self.get_bucket_index(entry.hash, entry.psl);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::hash::{BuildHasherDefault, Hasher};

    use super::{KeyStore, RESIZE_FACTOR, START_CAPACITY};

    #[derive(Default)]
    struct ConstantHasher;

    impl Hasher for ConstantHasher {
        fn finish(&self) -> u64 {
            0
        }

        fn write(&mut self, _bytes: &[u8]) {}
    }

    #[derive(Default)]
    struct WraparoundHasher;

    impl Hasher for WraparoundHasher {
        fn finish(&self) -> u64 {
            (START_CAPACITY - 2) as u64
        }

        fn write(&mut self, _bytes: &[u8]) {}
    }

    #[derive(Default)]
    struct IdentityHasher(u64);

    impl Hasher for IdentityHasher {
        fn finish(&self) -> u64 {
            self.0
        }

        fn write(&mut self, bytes: &[u8]) {
            let mut value = [0; 8];
            let len = bytes.len().min(value.len());
            value[..len].copy_from_slice(&bytes[..len]);
            self.0 = u64::from_ne_bytes(value);
        }

        fn write_u64(&mut self, value: u64) {
            self.0 = value;
        }
    }

    #[test]
    fn insert_makes_value_retrievable() {
        let mut store = KeyStore::default();

        assert_eq!(store.insert("key", "value"), None);

        assert_eq!(store.get(&"key"), Some(&"value"));
        assert_eq!(store.len, 1);
    }

    #[test]
    fn insert_replaces_existing_value() {
        let mut store = KeyStore::default();
        assert_eq!(store.insert("key", "old"), None);

        assert_eq!(store.insert("key", "new"), Some("old"));

        assert_eq!(store.get(&"key"), Some(&"new"));
        assert_eq!(store.len, 1);
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let store = KeyStore::<&str, &str>::default();

        assert_eq!(store.get(&"missing"), None);
    }

    #[test]
    fn del_removes_value_and_preserves_probe_cluster() {
        let mut store = KeyStore::with_hasher(BuildHasherDefault::<ConstantHasher>::default());
        store.insert("first", 1);
        store.insert("middle", 2);
        store.insert("last", 3);

        assert_eq!(store.del(&"middle"), Some(2));
        assert_eq!(store.get(&"middle"), None);
        assert_eq!(store.get(&"first"), Some(&1));
        assert_eq!(store.get(&"last"), Some(&3));
        assert_eq!(store.del(&"middle"), None);
        assert_eq!(store.len, 2);
    }

    #[test]
    fn del_removes_head_of_probe_cluster() {
        let mut store = KeyStore::with_hasher(BuildHasherDefault::<ConstantHasher>::default());
        store.insert("first", 1);
        store.insert("middle", 2);
        store.insert("last", 3);

        assert_eq!(store.del(&"first"), Some(1));

        assert_eq!(store.get(&"first"), None);
        assert_eq!(store.get(&"middle"), Some(&2));
        assert_eq!(store.get(&"last"), Some(&3));
        assert!(matches!(
            store.buckets[0].as_ref(),
            Some(entry) if entry.key == "middle" && entry.psl == 0
        ));
        assert!(matches!(
            store.buckets[1].as_ref(),
            Some(entry) if entry.key == "last" && entry.psl == 1
        ));
    }

    #[test]
    fn del_removes_tail_of_probe_cluster() {
        let mut store = KeyStore::with_hasher(BuildHasherDefault::<ConstantHasher>::default());
        store.insert("first", 1);
        store.insert("middle", 2);
        store.insert("last", 3);

        assert_eq!(store.del(&"last"), Some(3));

        assert_eq!(store.get(&"first"), Some(&1));
        assert_eq!(store.get(&"middle"), Some(&2));
        assert_eq!(store.get(&"last"), None);
        assert!(store.buckets[2].is_none());
    }

    #[test]
    fn deletion_stops_at_entry_in_its_ideal_bucket() {
        let mut store = KeyStore::with_hasher(BuildHasherDefault::<IdentityHasher>::default());
        store.insert(0_u64, "before");
        store.insert(1_u64, "start");
        store.insert(17_u64, "displaced");

        assert_eq!(store.del(&0), Some("before"));

        assert!(store.buckets[0].is_none());
        assert!(matches!(
            store.buckets[1].as_ref(),
            Some(entry) if entry.key == 1 && entry.psl == 0
        ));
        assert_eq!(store.get(&1), Some(&"start"));
        assert_eq!(store.get(&17), Some(&"displaced"));
    }

    #[test]
    fn resizes_at_load_threshold_and_preserves_entries() {
        let mut store = KeyStore::default();
        let resize_at = (START_CAPACITY as f32 * RESIZE_FACTOR).ceil() as usize;

        for key in 0..resize_at - 1 {
            store.insert(key, key * 10);
        }
        assert_eq!(store.buckets.len(), START_CAPACITY);

        store.insert(resize_at - 1, (resize_at - 1) * 10);

        assert_eq!(store.buckets.len(), START_CAPACITY * 2);
        assert_eq!(store.len, resize_at);
        for key in 0..resize_at {
            assert_eq!(store.get(&key), Some(&(key * 10)));
        }
    }

    #[test]
    fn multiple_resizes_preserve_colliding_entries() {
        let mut store = KeyStore::with_hasher(BuildHasherDefault::<ConstantHasher>::default());

        for key in 0..100 {
            store.insert(key, key * 10);
        }

        assert_eq!(store.buckets.len(), START_CAPACITY * 16);
        assert_eq!(store.len, 100);
        for key in 0..100 {
            assert_eq!(store.get(&key), Some(&(key * 10)));
        }
    }

    #[test]
    fn deletion_after_resize_preserves_remaining_entries() {
        let mut store = KeyStore::with_hasher(BuildHasherDefault::<ConstantHasher>::default());
        for key in 0..20 {
            store.insert(key, key * 10);
        }

        for key in [0, 7, 19] {
            assert_eq!(store.del(&key), Some(key * 10));
        }

        assert_eq!(store.len, 17);
        for key in 0..20 {
            if [0, 7, 19].contains(&key) {
                assert_eq!(store.get(&key), None);
            } else {
                assert_eq!(store.get(&key), Some(&(key * 10)));
            }
        }
    }

    #[test]
    fn insertion_reuses_space_after_deletion() {
        let mut store = KeyStore::with_hasher(BuildHasherDefault::<ConstantHasher>::default());
        store.insert("first", 1);
        store.insert("removed", 2);
        store.insert("last", 3);
        store.del(&"removed");

        store.insert("new", 4);

        assert_eq!(store.len, 3);
        assert_eq!(store.get(&"first"), Some(&1));
        assert_eq!(store.get(&"removed"), None);
        assert_eq!(store.get(&"last"), Some(&3));
        assert_eq!(store.get(&"new"), Some(&4));
    }

    #[test]
    fn insertion_uses_robin_hood_swapping_for_mixed_hashes() {
        let mut store = KeyStore::with_hasher(BuildHasherDefault::<IdentityHasher>::default());
        store.insert(1_u64, "first");
        store.insert(2_u64, "second");
        store.insert(17_u64, "displaced");

        assert!(matches!(
            store.buckets[1].as_ref(),
            Some(entry) if entry.key == 1 && entry.psl == 0
        ));
        assert!(matches!(
            store.buckets[2].as_ref(),
            Some(entry) if entry.key == 17 && entry.psl == 1
        ));
        assert!(matches!(
            store.buckets[3].as_ref(),
            Some(entry) if entry.key == 2 && entry.psl == 1
        ));
        assert_eq!(store.get(&1), Some(&"first"));
        assert_eq!(store.get(&2), Some(&"second"));
        assert_eq!(store.get(&17), Some(&"displaced"));
    }

    #[test]
    fn deletion_preserves_a_cluster_that_wraps_around() {
        let mut store = KeyStore::with_hasher(BuildHasherDefault::<WraparoundHasher>::default());
        store.insert("first", 1);
        store.insert("middle", 2);
        store.insert("last", 3);

        assert!(matches!(
            store.buckets[0].as_ref(),
            Some(entry) if entry.key == "last"
        ));
        assert_eq!(store.del(&"middle"), Some(2));

        assert_eq!(store.get(&"first"), Some(&1));
        assert_eq!(store.get(&"middle"), None);
        assert_eq!(store.get(&"last"), Some(&3));
        assert!(store.buckets[0].is_none());
    }

    #[test]
    fn randomized_operations_match_std_hash_map() {
        let mut store = KeyStore::default();
        let mut reference = HashMap::new();
        let mut random_state = 0x4d595df4d0f33173_u64;

        for _ in 0..2_000 {
            random_state = random_state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let key = random_state % 64;
            random_state = random_state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);

            match random_state % 3 {
                0 => {
                    let value = random_state;
                    store.insert(key, value);
                    reference.insert(key, value);
                }
                1 => assert_eq!(store.get(&key), reference.get(&key)),
                2 => assert_eq!(store.del(&key), reference.remove(&key)),
                _ => unreachable!(),
            }

            assert_eq!(store.len, reference.len());
            for candidate in 0_u64..64 {
                assert_eq!(store.get(&candidate), reference.get(&candidate));
            }
        }
    }
}
