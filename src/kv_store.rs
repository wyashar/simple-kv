use rustc_hash::FxBuildHasher;
use std::hash::{BuildHasher, Hash, Hasher};

const LOAD_FACTOR: f32 = 0.75;
const STARTING_CAPACITY: usize = 16;

pub struct KvStore<K, V>
where
    K: Hash + Eq,
    V: Clone + PartialEq,
{
    buckets: Box<[Option<Node<K, V>>]>,
    size: usize,
    hasher: FxBuildHasher,
}

pub struct Node<K, V>
where
    K: Hash,
    V: Clone + PartialEq,
{
    key: K,
    value: V,
    hash: usize,
    next: Option<Box<Node<K, V>>>,
}

impl<K, V> PartialEq for Node<K, V>
where
    K: Hash + Eq,
    V: Clone + PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.value == other.value
    }
}

impl<K, V> Eq for Node<K, V>
where
    K: Hash + Eq,
    V: Clone + PartialEq,
{
}

impl<K, V> Node<K, V>
where
    K: Hash + Eq,
    V: Clone + PartialEq,
{
    pub fn new(key: K, value: V, hash: usize) -> Self {
        Self {
            key: key,
            value: value,
            hash: hash,
            next: None,
        }
    }

    pub fn with_next(key: K, value: V, hash: usize, next: Node<K, V>) -> Self {
        Self {
            key: key,
            value: value,
            hash: hash,
            next: Some(Box::new(next)),
        }
    }
}

impl<K, V> KvStore<K, V>
where
    K: Hash + Eq,
    V: Clone + PartialEq,
{
    pub fn new() -> Self {
        Self {
            buckets: Box::new([const { None }; STARTING_CAPACITY]),
            size: 0,
            hasher: FxBuildHasher::default(),
        }
    }

    fn get_bucket_index(&self, hash: &usize) -> usize {
        hash & (self.buckets.len() - 1)
    }

    fn hash_key(&self, key: &K) -> usize {
        let mut hasher = self.hasher.build_hasher();
        key.hash(&mut hasher);
        hasher.finish() as usize
    }

    pub fn put(&mut self, key: K, value: V) {
        let hash: usize = self.hash_key(&key);
        let bucket_index = self.get_bucket_index(&hash);

        match self.buckets[bucket_index].take() {
            None => self.buckets[bucket_index] = Some(Node::new(key, value, hash)),
            Some(n) => {
                let mut prev: Option<&Node<K, V>> = None;
                let mut curr: Option<&Node<K, V>> = Some(&n);

                while curr != None {
                    let Some(val) = curr else {
                        continue;
                    };

                    if (val.hash == hash && val.key == key) {
                        prev.next = val.next
                    }
                }

                let entry: Node<K, V> = Node::with_next(key, value, hash, n);
                self.buckets[bucket_index] = Some(entry);
            }
        }
    }
}
