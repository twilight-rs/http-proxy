use std::{
    borrow::Borrow,
    collections::HashMap,
    future::poll_fn,
    hash::Hash,
    num::NonZero,
    sync::{Arc, Mutex},
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::sync::Notify;
use tokio_util::time::{DelayQueue, delay_queue};

/// A time-aware least recently used cache.
///
/// Entries are removed once their time-to-use (TTU) expires.
pub struct Tlru<K, V> {
    cap: NonZero<usize>,
    inner: Arc<Mutex<TlruInner<K, V>>>,
    notify: Arc<Notify>,
    ttu: Duration,
}

// INVARIANT: both collections contain the same keys.
struct TlruInner<K, V> {
    entries: HashMap<K, Entry<V>>,
    expirations: DelayQueue<K>,
}

#[derive(Clone)]
struct Entry<V> {
    expiration: delay_queue::Key,
    value: V,
}

impl<K, V> Tlru<K, V>
where
    K: Clone + Eq + Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Creates an empty `Tlru`.
    pub fn new(cap: NonZero<usize>, ttu: Duration) -> Self {
        let inner = TlruInner {
            entries: HashMap::new(),
            expirations: DelayQueue::new(),
        };
        let inner = Arc::new(Mutex::new(inner));
        let notify = Arc::new(Notify::new());

        tokio::spawn(reaper(Arc::clone(&notify), Arc::clone(&inner)));

        Self {
            cap,
            inner,
            notify,
            ttu,
        }
    }

    /// Inserts a key-value pair into the cache.
    ///
    /// If the cache is full, the least recently used entry is replaced.
    pub fn insert(&self, key: K, value: V) {
        let mut guard = self.inner.lock().unwrap();
        // Notify `reaper` that the cache is non-empty.
        self.notify.notify_waiters();

        if self.cap.get() == guard.len() {
            guard.remove_lru();
        }

        let expiration = guard.expirations.insert(key.clone(), self.ttu);
        guard.entries.insert(key, Entry { expiration, value });
    }

    /// Returns the value corresponding to the key.
    pub fn get<Q: ?Sized>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash,
    {
        let mut guard = self.inner.lock().unwrap();

        let Entry { expiration, value } = guard.entries.get(key)?.clone();
        guard.expirations.reset(&expiration, self.ttu);

        Some(value)
    }
}

impl<K, V> TlruInner<K, V>
where
    K: Eq + Hash,
{
    /// Returns the number of entries in the cache.
    fn len(&self) -> usize {
        debug_assert_eq!(self.entries.len(), self.expirations.len());
        self.entries.len()
    }

    /// Attempts to remove an expired entry.
    fn poll_expired(&mut self, cx: &mut Context<'_>) -> Poll<Option<V>> {
        let expired = ready!(self.expirations.poll_expired(cx));
        let entry = expired.map(|expired| self.entries.remove(expired.get_ref()).unwrap().value);

        Poll::Ready(entry)
    }

    /// Removes the least recently used entry.
    fn remove_lru(&mut self) -> Option<V> {
        let lru = self.expirations.remove(&self.expirations.peek()?);
        let entry = self.entries.remove(lru.get_ref()).unwrap().value;

        Some(entry)
    }
}

async fn reaper<K: Eq + Hash, V>(notify: Arc<Notify>, tlru: Arc<Mutex<TlruInner<K, V>>>) {
    loop {
        let expired = poll_fn(|cx| tlru.lock().unwrap().poll_expired(cx)).await;

        if expired.is_none() {
            // Wait until the cache is non-empty.
            notify.notified().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test(start_paused = true)]
    async fn tlru() {
        let tlru = Tlru::new(NonZero::new(2).unwrap(), Duration::from_secs(1));

        tlru.insert(1, 2);

        {
            let entry = tlru.get(&1).unwrap();
            assert_eq!(entry, 2);
        }

        sleep(Duration::from_secs(2)).await;
        assert!(tlru.get(&1).is_none());

        for i in 2..5 {
            tlru.insert(i, 0);

            // If we insert instantly after another,
            // upon inserting 4 it will remove either 2 or 3,
            // because they were inserted at the same time.
            //
            // For reproducibility, add a delay.
            sleep(Duration::from_millis(50)).await;
        }

        assert_eq!(tlru.inner.lock().unwrap().len(), 2);
        assert!(tlru.get(&2).is_none());
        assert!(tlru.get(&4).is_some());
    }
}
