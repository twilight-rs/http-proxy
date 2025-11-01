use crate::expiring_lru::{Builder, ExpiringLru};
use tokio::time::Duration;
use twilight_http_ratelimiting::InMemoryRatelimiter;

pub struct RatelimiterMap {
    default: InMemoryRatelimiter,
    default_token: String,
    inner: ExpiringLru<String, InMemoryRatelimiter>,
}

impl RatelimiterMap {
    pub fn new(mut default_token: String, timeout: Duration, max_size: Option<usize>) -> Self {
        let is_bot = default_token.starts_with("Bot ");
        let is_bearer = default_token.starts_with("Bearer ");

        // Make sure it is either a bot or bearer token, and assume it's a bot
        // token if no prefix is given
        if !is_bot && !is_bearer {
            default_token.insert_str(0, "Bot ");
        }

        let mut builder = Builder::new().expiration(timeout);

        if let Some(max_size) = max_size {
            builder = builder.max_size(max_size);
        }

        let inner = builder.build();

        let default = InMemoryRatelimiter::new();

        Self {
            default,
            default_token,
            inner,
        }
    }

    pub fn get_or_insert(&self, token: Option<&str>) -> (InMemoryRatelimiter, String) {
        if let Some(token) = token {
            if token == self.default_token {
                (self.default.clone(), self.default_token.clone())
            } else if let Some(entry) = self.inner.get(token) {
                (entry.value().clone(), token.to_string())
            } else {
                let ratelimiter = InMemoryRatelimiter::new();

                self.inner.insert(token.to_string(), ratelimiter.clone());

                (ratelimiter, token.to_string())
            }
        } else {
            (self.default.clone(), self.default_token.clone())
        }
    }
}
