use dashmap::{mapref::one::Ref, DashMap};
use futures_util::StreamExt;
use std::{borrow::Borrow, hash::Hash, marker::PhantomData, ops::Deref, sync::Arc, time::Duration};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_util::time::{delay_queue::Key, DelayQueue};
use tracing::debug;

pub struct Entry<V> {
    inner: V,
    decay_key: Key,
}

pub struct EntryRef<'a, K, V>(Ref<'a, K, Entry<V>>);

impl<'a, K, V> EntryRef<'a, K, V>
where
    K: Eq + Hash,
{
    pub fn value(&self) -> &V {
        &self.0.value().inner
    }
}

impl<'a, K, V> AsRef<V> for EntryRef<'a, K, V>
where
    K: Eq + Hash,
{
    fn as_ref(&self) -> &V {
        &self.0.value().inner
    }
}

impl<'a, K, V> Deref for EntryRef<'a, K, V>
where
    K: Eq + Hash,
{
    type Target = V;

    fn deref(&self) -> &Self::Target {
        &self.0.value().inner
    }
}

async fn decay_task<K, V>(
    map: Arc<DashMap<K, Entry<V>>>,
    expiration: Duration,
    mut rx: UnboundedReceiver<TimerUpdate<K, V>>,
) where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    let mut queue = DelayQueue::new();

    loop {
        tokio::select! {
            Some(key) = queue.next(), if !queue.is_empty() => {
                // An item expired in the queue, remove it from the map
                debug!("Removing expired entry from ratelimiter decay queue");
                map.remove(key.get_ref());
            }
            Some(msg) = rx.recv() => {
                match msg {
                    TimerUpdate::Add { key, value } => {
                        debug!("Adding entry to ratelimiter decay queue");
                        let decay_key = queue.insert(key.clone(), expiration);
                        let entry = Entry {
                            inner: value,
                            decay_key,
                        };
                        map.insert(key, entry);
                    },
                    TimerUpdate::Refresh { key } => {
                        debug!("Refreshing entry in ratelimiter decay queue");
                        // This will panic if the key is not present, therefore
                        // we check that in the calling end
                        queue.reset(&key, expiration);
                    },
                    TimerUpdate::RemoveLru => {
                        debug!("Removing least recently used item from ratelimiter decay queue");
                        if let Some(expired) = queue.peek().and_then(|key| queue.try_remove(&key)) {
                            map.remove(expired.get_ref());
                        }
                    }
                }
            },
            else => {
                // Channel has been closed by the other end, i.e. the ExpiringLru has
                // been dropped.
                break;
            }
        };
    }
}

enum TimerUpdate<K, V> {
    Add { key: K, value: V },
    Refresh { key: Key },
    RemoveLru,
}

pub struct ExpiringLru<K, V> {
    inner: Arc<DashMap<K, Entry<V>>>,
    decay_tx: UnboundedSender<TimerUpdate<K, V>>,
    max_size: Option<usize>,
}

impl<K, V> ExpiringLru<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    fn new(expiration: Duration, max_size: Option<usize>) -> Self {
        let inner = Arc::new(DashMap::new());
        let (decay_tx, decay_rx) = unbounded_channel();

        let this = Self {
            inner: inner.clone(),
            decay_tx,
            max_size,
        };

        tokio::spawn(decay_task(inner, expiration, decay_rx));

        this
    }

    pub fn insert(&self, key: K, value: V) {
        match self.max_size {
            Some(max_size) if max_size == 0 => return,
            Some(max_size) if self.len() >= max_size => {
                self.remove_lru();
            }
            _ => {}
        }

        _ = self.decay_tx.send(TimerUpdate::Add { key, value });
    }

    pub fn get<Q>(&self, key: &Q) -> Option<EntryRef<'_, K, V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let entry = self.inner.get(key)?;
        _ = self.decay_tx.send(TimerUpdate::Refresh {
            key: entry.decay_key,
        });

        Some(EntryRef(entry))
    }

    fn remove_lru(&self) {
        _ = self.decay_tx.send(TimerUpdate::RemoveLru);
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

pub struct Builder<K, V> {
    expiration: Duration,
    max_size: Option<usize>,

    _marker: PhantomData<(K, V)>,
}

const DEFAULT_EXPIRATION: Duration = Duration::from_secs(3600);

impl<K, V> Builder<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    pub const fn new() -> Self {
        Self {
            expiration: DEFAULT_EXPIRATION,
            max_size: None,
            _marker: PhantomData,
        }
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
        ExpiringLru::new(self.expiration, self.max_size)
    }
}

#[cfg(test)]
mod tests {
    use super::Builder;
    use tokio::time::{sleep, Duration};

    #[tokio::test(start_paused = true)]
    async fn test_lru() {
        let lru = Builder::new()
            .expiration(Duration::from_secs(1))
            .max_size(2)
            .build();

        lru.insert(1, 2);

        // The actual LRU cache insert is performed in a different
        // task and insert will return pre-emptively after notifying
        // the task of the insertion. In order to allow the task to run
        // and receive the insertion message, we have to yield back to the
        // runtime. The alternative would be making insert asynchronous and
        // wait on a oneshot channel, but there is no benefit to that
        // for our usecase.
        tokio::task::yield_now().await;

        {
            let entry = lru.get(&1).unwrap();
            assert_eq!(entry.value(), &2);
        }

        sleep(Duration::from_secs(2)).await;
        assert!(lru.get(&1).is_none());

        for i in 2..5 {
            lru.insert(i, 0);

            // If we insert instantly after another,
            // upon inserting 4 it will remove either 2 or 3,
            // because they were inserted at the same time.
            //
            // For reproducibility, add a delay.
            sleep(Duration::from_millis(50)).await;
        }

        assert_eq!(lru.len(), 2);
        assert!(lru.get(&2).is_none());
        assert!(lru.get(&4).is_some());
    }
}
