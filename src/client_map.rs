use dashmap::DashMap;
use std::{env::var, sync::Arc};
use tokio::time::{interval, Duration, Instant};
use tracing::{debug, warn};
use twilight_http::Client;

pub struct ClientMap {
    default: Client,
    max_size: Option<usize>,
    inner: Arc<DashMap<String, (Client, Instant)>>,
}

async fn reap_old_clients(map: Arc<DashMap<String, (Client, Instant)>>) {
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

        debug!("Done reaping timed out HTTP clients");
    }
}

impl ClientMap {
    pub fn new(default_client_token: String) -> Self {
        let max_size = var("CLIENT_CACHE_MAX_SIZE").map_or(None, |size| {
            if let Ok(size) = size.parse() {
                Some(size)
            } else {
                warn!("Unable to parse CLIENT_CACHE_MAX_SIZE, proceeding with defaults");
                None
            }
        });
        let inner = Arc::new(DashMap::new());
        let default = Client::new(default_client_token);

        tokio::spawn(reap_old_clients(inner.clone()));

        Self {
            default,
            max_size,
            inner,
        }
    }

    pub fn get(&self, token: Option<&str>) -> Client {
        if let Some(token) = token {
            match self.default.token() {
                Some(default_client_token) if default_client_token == token => self.default.clone(),
                _ => {
                    let access_time = Instant::now();
                    let maybe_entry = self.inner.get_mut(token);

                    if let Some(mut entry) = maybe_entry {
                        entry.1 = access_time;
                        entry.0.clone()
                    } else {
                        if let Some(max_size) = self.max_size {
                            if self.inner.len() >= max_size && max_size > 0 {
                                let key = {
                                    let first_entry = self.inner.iter().next().unwrap();
                                    let oldest_entry =
                                        self.inner.iter().fold(first_entry, |old, next| {
                                            if old.1 > next.1 {
                                                next
                                            } else {
                                                old
                                            }
                                        });

                                    oldest_entry.key().to_string()
                                };
                                self.inner.remove(&key);
                                debug!("Removed oldest entry from HTTP client cache");
                            }
                        }

                        let client = Client::new(token.to_string());
                        self.inner
                            .insert(token.to_string(), (client.clone(), access_time));
                        client
                    }
                }
            }
        } else {
            self.default.clone()
        }
    }
}
