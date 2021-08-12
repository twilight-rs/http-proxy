use dashmap::{mapref::multiple::RefMulti, DashMap};
use std::{env::var, sync::Arc};
use tokio::time::{interval, Duration, Instant};
use tracing::{debug, warn};
use twilight_http::ratelimiting::Ratelimiter;

pub struct RatelimiterMap {
    default: Ratelimiter,
    default_token: String,
    max_size: Option<usize>,
    inner: Arc<DashMap<String, (Ratelimiter, Instant)>>,
}

async fn reap_old_ratelimiters(map: Arc<DashMap<String, (Ratelimiter, Instant)>>) {
    let client_reap_interval =
        Duration::from_secs(var("CLIENT_REAP_INTERVAL").map_or(600, |timeout| {
            if let Ok(timeout_secs) = timeout.parse() {
                timeout_secs
            } else {
                warn!("Unable to parse CLIENT_REAP_INTERVAL, proceeding with defaults");
                600
            }
        }));

    let client_decay_timeout =
        Duration::from_secs(var("CLIENT_DECAY_TIEOUT").map_or(3600, |timeout| {
            if let Ok(timeout_secs) = timeout.parse() {
                timeout_secs
            } else {
                warn!("Unable to parse CLIENT_DECAY_TIMEOUT, proceeding with defaults");
                3600
            }
        }));

    let mut interval = interval(client_reap_interval);

    loop {
        interval.tick().await;
        let right_now = Instant::now();

        map.retain(|_, (_, last_used)| *last_used + client_decay_timeout > right_now);

        debug!("Done reaping timed out HTTP ratelimiters");
    }
}

impl RatelimiterMap {
    pub fn new(mut default_token: String) -> Self {
        let is_bot = default_token.starts_with("Bot ");
        let is_bearer = default_token.starts_with("Bearer ");

        // Make sure it is either a bot or bearer token, and assume it's a bot
        // token if no prefix is given
        if !is_bot && !is_bearer {
            default_token.insert_str(0, "Bot ");
        }

        let max_size = var("CLIENT_CACHE_MAX_SIZE").map_or(None, |size| {
            if let Ok(size) = size.parse() {
                Some(size)
            } else {
                warn!("Unable to parse CLIENT_CACHE_MAX_SIZE, proceeding with defaults");
                None
            }
        });

        let inner = Arc::new(DashMap::new());
        let default = Ratelimiter::new();

        tokio::spawn(reap_old_ratelimiters(inner.clone()));

        Self {
            default,
            default_token,
            max_size,
            inner,
        }
    }

    fn lru(&self) -> Option<RefMulti<String, (Ratelimiter, Instant)>> {
        self.inner.iter().next().map(|first_entry| {
            self.inner.iter().fold(
                first_entry,
                |old, next| {
                    if old.1 > next.1 {
                        next
                    } else {
                        old
                    }
                },
            )
        })
    }

    pub fn get(&self, token: Option<&str>) -> (Ratelimiter, String) {
        if let Some(token) = token {
            if token == self.default_token {
                (self.default.clone(), self.default_token.clone())
            } else {
                let access_time = Instant::now();
                let maybe_entry = self.inner.get_mut(token);

                if let Some(mut entry) = maybe_entry {
                    entry.1 = access_time;
                    (entry.0.clone(), token.to_string())
                } else {
                    if self
                        .max_size
                        .filter(|max_size| self.inner.len() >= *max_size && max_size > &0)
                        .is_some()
                    {
                        let key = self.lru().unwrap().key().to_string();

                        self.inner.remove(&key);
                        debug!("Removed oldest entry from HTTP ratelimiter cache");
                    }

                    let ratelimiter = Ratelimiter::new();

                    if self.max_size.filter(|max| max != &0).is_some() {
                        self.inner
                            .insert(token.to_string(), (ratelimiter.clone(), access_time));
                    }

                    (ratelimiter, token.to_string())
                }
            }
        } else {
            (self.default.clone(), self.default_token.clone())
        }
    }
}
