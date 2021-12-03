use dashmap::{mapref::multiple::RefMulti, DashMap};
use std::{env, str::FromStr, sync::Arc};
use tokio::time::{interval, Duration, Instant};
use tracing::{debug, warn};
use twilight_http_ratelimiting::InMemoryRatelimiter;

pub struct RatelimiterMap {
    default: InMemoryRatelimiter,
    default_token: String,
    max_size: Option<usize>,
    inner: Arc<DashMap<String, (InMemoryRatelimiter, Instant)>>,
}

fn parse_env<T: FromStr>(key: &str) -> Option<T> {
    env::var_os(key).and_then(|value| match value.into_string() {
        Ok(s) => {
            if let Ok(t) = s.parse() {
                Some(t)
            } else {
                warn!("Unable to parse {}, proceeding with defaults", key);
                None
            }
        }
        Err(s) => {
            warn!("{} is not UTF-8: {:?}", key, s);
            None
        }
    })
}

async fn reap_old_ratelimiters(map: Arc<DashMap<String, (InMemoryRatelimiter, Instant)>>) {
    let client_reap_interval =
        Duration::from_secs(parse_env("CLIENT_REAP_INTERVAL").unwrap_or(600));

    let client_decay_timeout =
        Duration::from_secs(parse_env("CLIENT_DECAY_TIMEOUT").unwrap_or(3600));

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

        let max_size = parse_env("CLIENT_CACHE_MAX_SIZE");

        let inner = Arc::new(DashMap::new());
        let default = InMemoryRatelimiter::new();

        tokio::spawn(reap_old_ratelimiters(inner.clone()));

        Self {
            default,
            default_token,
            max_size,
            inner,
        }
    }

    fn lru(&self) -> Option<RefMulti<String, (InMemoryRatelimiter, Instant)>> {
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

    pub fn get_or_insert(&self, token: Option<&str>) -> (InMemoryRatelimiter, String) {
        if let Some(token) = token {
            if token == self.default_token {
                (self.default.clone(), self.default_token.clone())
            } else {
                let access_time = Instant::now();

                if let Some(mut entry) = self.inner.get_mut(token) {
                    entry.1 = access_time;
                    (entry.0.clone(), token.to_string())
                } else {
                    if self
                        .max_size
                        .filter(|max_size| self.inner.len() >= *max_size && max_size > &0)
                        .is_some()
                    {
                        let key = self
                            .lru()
                            .expect("Length of inner map is guaranteed to be greater than 0")
                            .key()
                            .clone();

                        self.inner.remove(&key);
                        debug!("Removed oldest entry from HTTP ratelimiter cache");
                    }

                    let ratelimiter = InMemoryRatelimiter::new();

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
