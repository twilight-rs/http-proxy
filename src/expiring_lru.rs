use dashmap::{mapref::one::Ref, DashMap};
use futures_util::StreamExt;
use std::{borrow::Borrow, hash::Hash, marker::PhantomData, ops::Deref, sync::Arc, time::Duration};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot,
};
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
    #[allow(unused)]
    pub fn key(&self) -> &K {
        self.0.key()
    }

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
    map: ExpiringLru<K, V>,
    expiration: Duration,
    mut rx: UnboundedReceiver<TimerUpdate<K>>,
) where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    let mut queue = DelayQueue::new();

    loop {
        tokio::select! {
            expired = queue.next(), if !queue.is_empty() => {
                // An item expired in the queue, remove it from the map
                if let Some(key) = expired {
                    debug!("Removing expired entry from ratelimiter decay queue");
                    map.remove(key.get_ref());
                } else {
                    // This should not occur because we only poll next if the queue is not empty
                    break;
                }
            }
            msg = rx.recv() => {
                if let Some(msg) = msg {
                    match msg {
                        TimerUpdate::Add { map_key, return_key_to } => {
                            debug!("Adding entry to ratelimiter decay queue");
                            let key = queue.insert(map_key, expiration);
                            let _ = return_key_to.send(key);
                        },
                        TimerUpdate::Refresh { key } => {
                            debug!("Refreshing entry in ratelimiter decay queue");
                            // This will panic if the key is not present, therefore
                            // we check that in the calling end
                            queue.reset(&key, expiration);
                        },
                        TimerUpdate::Remove { key } => {
                            debug!("Removing entry in ratelimiter decay queue");
                            queue.try_remove(&key);
                        }
                        TimerUpdate::RemoveLru { return_map_key_to } => {
                            debug!("Removing least recently used item from ratelimiter decay queue");
                            if let Some(expired) = queue.peek().and_then(|key| queue.try_remove(&key)) {
                                let _ = return_map_key_to.send(Some(expired.into_inner()));
                            } else {
                                let _ = return_map_key_to.send(None);
                            };
                        }
                    }
                } else {
                    // Channel closed by other end
                    break;
                }
            }
        };
    }
}

enum TimerUpdate<K> {
    Add {
        map_key: K,
        return_key_to: oneshot::Sender<Key>,
    },
    Refresh {
        key: Key,
    },
    Remove {
        key: Key,
    },
    RemoveLru {
        return_map_key_to: oneshot::Sender<Option<K>>,
    },
}

pub struct ExpiringLru<K, V> {
    inner: Arc<DashMap<K, Entry<V>>>,
    decay_tx: UnboundedSender<TimerUpdate<K>>,
    max_size: Option<usize>,
}

impl<K, V> Clone for ExpiringLru<K, V> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            decay_tx: self.decay_tx.clone(),
            max_size: self.max_size,
        }
    }
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
            inner,
            decay_tx,
            max_size,
        };

        tokio::spawn(decay_task(this.clone(), expiration, decay_rx));

        this
    }

    pub async fn insert(&self, key: K, value: V) {
        match self.max_size {
            Some(max_size) if max_size == 0 => return,
            Some(max_size) if self.len() >= max_size => {
                self.remove_lru().await;
            }
            _ => {}
        }

        let (tx, rx) = oneshot::channel();

        if self
            .decay_tx
            .send(TimerUpdate::Add {
                map_key: key.clone(),
                return_key_to: tx,
            })
            .is_ok()
        {
            if let Ok(decay_key) = rx.await {
                let entry = Entry {
                    inner: value,
                    decay_key,
                };
                self.inner.insert(key, entry);
            }
        }
    }

    pub fn get<Q>(&self, key: &Q) -> Option<EntryRef<'_, K, V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.inner.get(key).map(|entry| {
            let _ = self.decay_tx.send(TimerUpdate::Refresh {
                key: entry.decay_key,
            });

            EntryRef(entry)
        })
    }

    async fn remove_lru(&self) {
        let (tx, rx) = oneshot::channel();

        let _ = self.decay_tx.send(TimerUpdate::RemoveLru {
            return_map_key_to: tx,
        });

        if let Ok(Some(key)) = rx.await {
            self.remove(&key);
        }
    }

    pub fn remove(&self, key: &K) -> Option<(K, Entry<V>)> {
        if let Some((key, item)) = self.inner.remove(key) {
            let _ = self.decay_tx.send(TimerUpdate::Remove {
                key: item.decay_key,
            });
            Some((key, item))
        } else {
            None
        }
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

    #[tokio::test]
    async fn test_lru() {
        let lru = Builder::new()
            .expiration(Duration::from_secs(1))
            .max_size(2)
            .build();

        lru.insert(1, 2).await;

        {
            let entry = lru.get(&1).unwrap();
            assert_eq!(entry.value(), &2);
        }

        sleep(Duration::from_secs(2)).await;
        assert!(lru.get(&1).is_none());

        for i in 2..5 {
            lru.insert(i, 0).await;
            // If we insert instantly after another,
            // upon inserting 4 it will remove either 2 or 3,
            // because they were inserted at the same time.
            // For reproducibility, add a delay.
            sleep(Duration::from_millis(50)).await;
        }

        assert_eq!(lru.len(), 2);
        assert!(lru.get(&2).is_none());
        assert!(lru.get(&4).is_some());
    }
}
