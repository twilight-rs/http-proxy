use dashmap::{
    mapref::{multiple::RefMulti, one::RefMut},
    DashMap,
};
use std::{borrow::Borrow, hash::Hash, marker::PhantomData, ops::Deref, sync::Arc, time::Duration};
use tokio::time::{interval, Instant};
use tracing::debug;

pub struct Entry<V> {
    inner: V,
    last_used: Instant,
}

enum EntryRefInner<'a, K, V> {
    RefMut(RefMut<'a, K, Entry<V>>),
    RefMulti(RefMulti<'a, K, Entry<V>>),
}

pub struct EntryRef<'a, K, V>(EntryRefInner<'a, K, V>);

impl<'a, K, V> EntryRef<'a, K, V>
where
    K: Eq + Hash,
{
    pub fn key(&self) -> &K {
        match &self.0 {
            EntryRefInner::RefMut(inner) => inner.key(),
            EntryRefInner::RefMulti(inner) => inner.key(),
        }
    }

    pub fn value(&self) -> &V {
        match &self.0 {
            EntryRefInner::RefMut(inner) => &inner.value().inner,
            EntryRefInner::RefMulti(inner) => &inner.value().inner,
        }
    }

    #[allow(unused)]
    pub fn last_used(&self) -> &Instant {
        match &self.0 {
            EntryRefInner::RefMut(inner) => &inner.value().last_used,
            EntryRefInner::RefMulti(inner) => &inner.value().last_used,
        }
    }
}

impl<'a, K, V> AsRef<V> for EntryRef<'a, K, V>
where
    K: Eq + Hash,
{
    fn as_ref(&self) -> &V {
        match &self.0 {
            EntryRefInner::RefMut(inner) => &inner.value().inner,
            EntryRefInner::RefMulti(inner) => &inner.value().inner,
        }
    }
}

impl<'a, K, V> Deref for EntryRef<'a, K, V>
where
    K: Eq + Hash,
{
    type Target = V;

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            EntryRefInner::RefMut(inner) => &inner.value().inner,
            EntryRefInner::RefMulti(inner) => &inner.value().inner,
        }
    }
}

async fn expire_entries<K, V>(map: ExpiringLru<K, V>, reap_interval: Duration, expiration: Duration)
where
    K: Eq + Hash,
{
    let mut interval = interval(reap_interval);

    loop {
        interval.tick().await;

        let right_now = Instant::now();
        map.inner
            .retain(|_, entry| entry.last_used + expiration > right_now);

        debug!("Done reaping timed out HTTP ratelimiters");
    }
}

pub struct ExpiringLru<K, V> {
    inner: Arc<DashMap<K, Entry<V>>>,
    max_size: Option<usize>,
}

impl<K, V> Clone for ExpiringLru<K, V> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            max_size: self.max_size,
        }
    }
}

impl<K, V> ExpiringLru<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    fn new(reap_interval: Duration, expiration: Duration, max_size: Option<usize>) -> Self {
        let inner = Arc::new(DashMap::new());

        let this = Self { inner, max_size };

        tokio::spawn(expire_entries(this.clone(), reap_interval, expiration));

        this
    }

    pub fn insert(&self, key: K, value: V) {
        match self.max_size {
            Some(max_size) if max_size == 0 => return,
            Some(max_size) if self.len() >= max_size => {
                if let Some(lru) = self.get_lru() {
                    let key = lru.key().clone();
                    // We can't hold any references when removing something from the map
                    drop(lru);
                    self.remove(&key);

                    debug!("Removed least recently used entry from expiring LRU");
                }
            }
            _ => {}
        }

        let last_used = Instant::now();
        let entry = Entry {
            inner: value,
            last_used,
        };
        self.inner.insert(key, entry);
    }

    pub fn get<Q>(&self, key: &Q) -> Option<EntryRef<'_, K, V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.inner.get_mut(key).map(|mut entry| {
            entry.last_used = Instant::now();

            EntryRef(EntryRefInner::RefMut(entry))
        })
    }

    pub fn get_lru(&self) -> Option<EntryRef<'_, K, V>> {
        self.inner
            .iter()
            .next()
            .map(|first_entry| {
                self.inner.iter().fold(first_entry, |old, next| {
                    if old.last_used > next.last_used {
                        next
                    } else {
                        old
                    }
                })
            })
            .map(|entry| EntryRef(EntryRefInner::RefMulti(entry)))
    }

    pub fn remove(&self, key: &K) -> Option<(K, Entry<V>)> {
        self.inner.remove(key)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

pub struct Builder<K, V> {
    reap_interval: Duration,
    expiration: Duration,
    max_size: Option<usize>,

    _marker: PhantomData<(K, V)>,
}

const DEFAULT_REAP_INTERVAL: Duration = Duration::from_secs(600);
const DEFAULT_EXPIRATION: Duration = Duration::from_secs(3600);

impl<K, V> Builder<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    pub const fn new() -> Self {
        Self {
            reap_interval: DEFAULT_REAP_INTERVAL,
            expiration: DEFAULT_EXPIRATION,
            max_size: None,
            _marker: PhantomData,
        }
    }

    pub const fn reap_interval(mut self, interval: Duration) -> Self {
        self.reap_interval = interval;

        self
    }

    pub const fn expiration(mut self, expiration: Duration) -> Self {
        self.expiration = expiration;

        self
    }

    pub const fn max_size(mut self, size: usize) -> Self {
        self.max_size = Some(size);

        self
    }

    pub fn build(self) -> ExpiringLru<K, V> {
        ExpiringLru::new(self.reap_interval, self.expiration, self.max_size)
    }
}

#[cfg(test)]
mod tests {
    use super::Builder;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_lru() {
        let lru = Builder::new()
            .expiration(Duration::from_secs(1))
            .reap_interval(Duration::from_millis(500))
            .max_size(2)
            .build();

        lru.insert(1, 2);

        // Ref has to be dropped to allow cleanup task to run!
        // This is a huge downside of the current implementation,
        // it uses get_mut and therefore easily deadlocks, for example
        // if you remove this scope.
        {
            let entry = lru.get(&1).unwrap();
            assert_eq!(entry.value(), &2);
        }
        sleep(Duration::from_secs(2)).await;
        assert!(lru.get(&1).is_none());

        for i in 2..5 {
            lru.insert(i, 0);
        }

        assert_eq!(lru.len(), 2);
    }
}
