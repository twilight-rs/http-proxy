use crate::tlru::Tlru;
use std::num::NonZero;
use tokio::time::Duration;
use twilight_http_ratelimiting::RateLimiter;

pub struct RatelimiterMap {
    default: RateLimiter,
    default_token: String,
    inner: Tlru<String, RateLimiter>,
}

impl RatelimiterMap {
    pub fn new(mut default_token: String, timeout: Duration, cap: NonZero<usize>) -> Self {
        let is_bot = default_token.starts_with("Bot ");
        let is_bearer = default_token.starts_with("Bearer ");

        // Make sure it is either a bot or bearer token, and assume it's a bot
        // token if no prefix is given
        if !is_bot && !is_bearer {
            default_token.insert_str(0, "Bot ");
        }

        let inner = Tlru::new(cap, timeout);

        let default = RateLimiter::default();

        Self {
            default,
            default_token,
            inner,
        }
    }

    pub fn get_or_insert(&self, token: Option<&str>) -> (RateLimiter, String) {
        if let Some(token) = token {
            if token == self.default_token {
                (self.default.clone(), self.default_token.clone())
            } else if let Some(entry) = self.inner.get(token) {
                (entry, token.to_owned())
            } else {
                let ratelimiter = RateLimiter::default();

                self.inner.insert(token.to_owned(), ratelimiter.clone());

                (ratelimiter, token.to_owned())
            }
        } else {
            (self.default.clone(), self.default_token.clone())
        }
    }
}
