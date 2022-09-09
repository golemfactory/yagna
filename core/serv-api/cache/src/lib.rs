use futures::Future;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;
use std::pin::Pin;
use std::time::{Duration, SystemTime};

pub trait ValueResolver {
    type Key: Clone;
    type Value: Clone;
    type Error;

    fn resolve<'a>(
        &self,
        key: &Self::Key,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Self::Value>, Self::Error>> + 'a>>;
}

pub struct AutoResolveCache<R>
where
    R: ValueResolver,
    <R as ValueResolver>::Key: Eq + Hash + std::cmp::PartialOrd + std::fmt::Debug,
{
    inner: TtlCache<<R as ValueResolver>::Key, Option<<R as ValueResolver>::Value>>,
    resolver: R,
}

impl<R> AutoResolveCache<R>
where
    R: ValueResolver,
    <R as ValueResolver>::Key: Eq + Hash + std::cmp::PartialOrd + std::fmt::Debug,
    <R as ValueResolver>::Error: std::fmt::Debug,
{
    pub fn new(ttl: Duration, capacity: usize, resolver: R) -> Self {
        Self {
            inner: TtlCache::new(ttl, capacity),
            resolver,
        }
    }

    #[inline(always)]
    pub fn get(
        &self,
        key: &<R as ValueResolver>::Key,
    ) -> Option<Option<<R as ValueResolver>::Value>> {
        self.inner.get(key)
    }

    pub async fn resolve(
        &mut self,
        key: &<R as ValueResolver>::Key,
    ) -> Option<<R as ValueResolver>::Value> {
        match self.resolver.resolve(key).await {
            Ok(v) => {
                self.inner.insert(key.clone(), v.clone());
                v
            }
            Err(e) => {
                log::error!("Error resolving key '{:?}': {:?}", key, e);
                None
            }
        }
    }
}

impl<R> Default for AutoResolveCache<R>
where
    R: ValueResolver + Default,
    <R as ValueResolver>::Key: Eq + Hash + std::cmp::PartialOrd + std::fmt::Debug,
    <R as ValueResolver>::Error: std::fmt::Debug,
{
    fn default() -> Self {
        Self::new(Duration::from_secs(2), 1024, R::default())
    }
}

pub struct TtlCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    map: HashMap<K, (SystemTime, V)>,
    ord: BinaryHeap<Reverse<KeyTimeEntry<K>>>,
    ttl: Duration,
    capacity: usize,
}

impl<K, V> TtlCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            ord: BinaryHeap::new(),
            ttl,
            capacity,
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        self.map
            .get(key)
            .and_then(|entry| (entry.0 + self.ttl >= SystemTime::now()).then(|| entry.1.clone()))
    }

    pub fn insert(&mut self, key: K, value: V) {
        let now = SystemTime::now();

        if self.ord.len() == self.capacity {
            let entry = self.ord.pop().unwrap();
            self.map.remove(&entry.0.key);
        }

        self.ord.push(Reverse(KeyTimeEntry {
            time: now,
            key: key.clone(),
        }));
        self.map.insert(key, (now, value));
    }
}

#[derive(Clone, Debug)]
struct KeyTimeEntry<K: Clone> {
    key: K,
    time: SystemTime,
}

impl<K: Clone> PartialEq for KeyTimeEntry<K> {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}

impl<K: Clone> Eq for KeyTimeEntry<K> {}

impl<K: Clone> PartialOrd for KeyTimeEntry<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.time.partial_cmp(&other.time)
    }
}

impl<K: Clone> Ord for KeyTimeEntry<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.time > other.time {
            Ordering::Greater
        } else if self.time == other.time {
            Ordering::Equal
        } else {
            Ordering::Less
        }
    }
}
