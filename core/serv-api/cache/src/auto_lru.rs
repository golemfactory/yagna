use futures::Future;
use lru_time_cache::LruCache;
use std::pin::Pin;
use std::time::{Duration, SystemTime};

pub trait ValueResolver {
    type Key;
    type Value;
    type Error;

    fn resolve<'a>(
        &self,
        key: &Self::Key,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Self::Value>, Self::Error>> + 'a>>;
}

pub struct TtlOrdEntry<V> {
    time: SystemTime,
    value: V,
}

impl<V> PartialEq for TtlOrdEntry<V> {}

pub struct TtlCache<K, V>
where
    K: Eq + Hash,
    V: Clone,
{
    map: HashMap<K, (SystemTime, V)>,
    ord: BinaryHeap<TtlOrdEntry>,
}

pub struct AutoResolveLruCache<R>
where
    R: ValueResolver,
    <R as ValueResolver>::Key: Clone + std::cmp::Ord + std::fmt::Debug,
{
    inner: LruCache<<R as ValueResolver>::Key, Option<<R as ValueResolver>::Value>>,
    resolver: R,
}

impl<R> AutoResolveLruCache<R>
where
    R: ValueResolver,
    <R as ValueResolver>::Key: Clone + std::cmp::Ord + std::fmt::Debug,
    <R as ValueResolver>::Error: std::fmt::Debug,
{
    pub fn new(ttl: Duration, capacity: usize, resolver: R) -> Self {
        Self {
            inner: LruCache::with_expiry_duration_and_capacity(ttl, capacity),
            resolver,
        }
    }

    pub async fn get(
        &mut self,
        key: &<R as ValueResolver>::Key,
    ) -> Option<&<R as ValueResolver>::Value> {
        let resolver = &mut self.resolver;
        let inner = &mut self.inner;

        if !inner.contains_key(key) {
            match resolver.resolve(key).await {
                Ok(v) => {
                    inner.insert(key.clone(), v);
                }
                Err(e) => log::error!("Error resolving key '{:?}': {:?}", key, e),
            }
        }

        match inner.get(key) {
            Some(value) => value.as_ref(),
            _ => None,
        }
    }

    #[inline(always)]
    pub fn insert(
        &mut self,
        key: &<R as ValueResolver>::Key,
        value: Option<<R as ValueResolver>::Value>,
    ) -> Option<Option<<R as ValueResolver>::Value>> {
        self.inner.insert(key.clone(), value)
    }

    #[inline(always)]
    pub fn remove(
        &mut self,
        key: &<R as ValueResolver>::Key,
    ) -> Option<Option<<R as ValueResolver>::Value>> {
        self.inner.remove(&key)
    }
}

impl<R> Default for AutoResolveLruCache<R>
where
    R: ValueResolver + Default,
    <R as ValueResolver>::Key: Clone + std::cmp::Ord + std::fmt::Debug,
    <R as ValueResolver>::Error: std::fmt::Debug,
{
    fn default() -> Self {
        Self::new(Duration::from_secs(2), 1024, R::default())
    }
}
