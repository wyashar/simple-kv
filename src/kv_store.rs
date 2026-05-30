const LOAD_FACTOR: f32 = 0.75;

pub struct KvStore<K, V> {
    buckets: Box<[Option<Node<K, V>>]>,
    size: usize,
}

struct Node<K, V> {
    key: K,
    value: V,
    hash: usize,
    next: Option<Box<Node<K, V>>>,
}

impl<K, V> KvStore<K, V> {}
