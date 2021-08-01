use dashmap::DashMap;
use std::sync::Arc;
use tokio::time::{interval, Duration, Instant};
use twilight_http::Client;

pub struct ClientMap {
    default: Client,
    inner: Arc<DashMap<String, (Client, Instant)>>,
}

async fn eliminate_old_clients(map: Arc<DashMap<String, (Client, Instant)>>) {
    let one_hour = Duration::from_secs(3600);
    let mut interval = interval(one_hour);
    loop {
        interval.tick().await;
        let right_now = Instant::now();

        map.retain(|_, (_, last_used)| *last_used + one_hour > right_now);
    }
}

impl ClientMap {
    pub fn new(default_client_token: String) -> Self {
        let inner = Arc::new(DashMap::new());
        let default = Client::new(default_client_token);

        tokio::spawn(eliminate_old_clients(inner.clone()));

        Self { default, inner }
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
                        let client = Client::new(token);
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
