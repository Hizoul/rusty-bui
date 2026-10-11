//! The one place requests leave the machine.
//!
//! Every provider call goes through [`HttpClient`], which:
//!
//! * answers from the on-disk [`ResponseCache`] when the entry is still fresh,
//! * refuses to send anything at all in offline mode,
//! * serves a stale cached response when a request fails or the network is down,
//! * enforces a minimum gap between requests and a per-minute budget, so a burst
//!   can never leave this process,
//! * counts network hits, cache hits and stale serves, so callers and tests can
//!   prove it.
//!
//! The endpoints are free open data; this is the difference between a good
//! citizen and an IP ban.

use super::{provider_log, ProviderError, ProviderResult};
use crate::cache::{request_key, ResponseCache, RESPONSE_MAX_STALE_SECONDS};
use crate::USER_AGENT;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Where a response came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Network,
    Cache,
    StaleCache,
}

/// Response counters for one client.
#[derive(Debug, Default, Clone)]
pub struct FetchStats {
    network: Arc<AtomicU64>,
    cache: Arc<AtomicU64>,
    stale: Arc<AtomicU64>,
}

impl FetchStats {
    /// `(network, cache, stale)`
    pub fn snapshot(&self) -> (u64, u64, u64) {
        (
            self.network.load(Ordering::Relaxed),
            self.cache.load(Ordering::Relaxed),
            self.stale.load(Ordering::Relaxed),
        )
    }

    fn record(&self, source: Source) {
        match source {
            Source::Network => &self.network,
            Source::Cache => &self.cache,
            Source::StaleCache => &self.stale,
        }
        .fetch_add(1, Ordering::Relaxed);
    }
}

/// Politeness gate: at least `min_gap` between two requests, and at most
/// `budget_per_minute` requests in any rolling minute.
struct Gate {
    last_request: Option<Instant>,
    window_start: Instant,
    window_count: u32,
}

pub struct HttpClient {
    provider: &'static str,
    http: reqwest::Client,
    cache: ResponseCache,
    min_gap: Duration,
    budget_per_minute: u32,
    gate: Mutex<Gate>,
    stats: FetchStats,
}

impl HttpClient {
    /// `min_gap` of 200 ms with a 60/minute budget keeps a 24-request KNMI
    /// nowcast at about five seconds and a repeated call free.
    pub fn new(provider: &'static str, cache: ResponseCache) -> ProviderResult<Self> {
        Self::with_policy(provider, cache, Duration::from_millis(200), 60)
    }

    pub fn with_policy(
        provider: &'static str,
        cache: ResponseCache,
        min_gap: Duration,
        budget_per_minute: u32,
    ) -> ProviderResult<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|source| ProviderError::Http { provider, source })?;

        Ok(Self {
            provider,
            http,
            cache,
            min_gap,
            budget_per_minute,
            gate: Mutex::new(Gate {
                last_request: None,
                window_start: Instant::now(),
                window_count: 0,
            }),
            stats: FetchStats::default(),
        })
    }

    pub fn cache(&self) -> &ResponseCache {
        &self.cache
    }

    pub fn stats(&self) -> FetchStats {
        self.stats.clone()
    }

    pub fn provider(&self) -> &'static str {
        self.provider
    }

    /// GET `url` with `query`, honouring the cache and the politeness gate.
    ///
    /// `fresh_for` is how long a cached response may be reused without asking the
    /// provider again; `header` carries an optional bearer-style credential (the
    /// KNMI API key).
    pub async fn get_text(
        &self,
        url: &str,
        query: &[(&str, String)],
        header: Option<(&str, &str)>,
        fresh_for: Duration,
    ) -> ProviderResult<String> {
        let key = request_key(url, query);

        if let Some(body) = self.cache.get(&key, fresh_for) {
            self.stats.record(Source::Cache);
            provider_log!("{}: cache hit for {}", self.provider, short_url(url));
            return Ok(body);
        }

        let max_stale = Duration::from_secs(RESPONSE_MAX_STALE_SECONDS as u64);
        if self.cache.is_offline() {
            if let Some(body) = self.cache.get_stale(&key, max_stale) {
                self.stats.record(Source::StaleCache);
                provider_log!("{}: offline, serving stale {}", self.provider, short_url(url));
                return Ok(body);
            }
            return Err(ProviderError::Unavailable {
                provider: self.provider,
                reason: "offline and no cached response for this request".to_string(),
            });
        }

        self.wait_for_slot().await;

        let mut request = self.http.get(url).query(query);
        if let Some((name, value)) = header {
            request = request.header(name, value);
        }

        let outcome = request.send().await;

        match outcome {
            Ok(response) => {
                let status = response.status();
                match response.text().await {
                    Ok(body) if status.is_success() => {
                        self.cache.put(&key, url, &body);
                        self.stats.record(Source::Network);
                        Ok(body)
                    }
                    Ok(body) => {
                        provider_log!(
                            "{}: HTTP {status} for {} — trying stale cache",
                            self.provider,
                            short_url(url)
                        );
                        self.stale_or_error(&key, max_stale, || {
                            ProviderError::Unavailable {
                                provider: self.provider,
                                reason: format!(
                                    "HTTP {status}: {}",
                                    body.chars().take(160).collect::<String>()
                                ),
                            }
                        })
                    }
                    Err(source) => {
                        provider_log!("{}: body read failed: {source}", self.provider);
                        self.stale_or_error(&key, max_stale, || ProviderError::Http {
                            provider: self.provider,
                            source,
                        })
                    }
                }
            }
            Err(source) => {
                provider_log!("{}: request failed: {source}", self.provider);
                self.stale_or_error(&key, max_stale, || ProviderError::Http {
                    provider: self.provider,
                    source,
                })
            }
        }
    }

    /// Falls back to a stale cached response, otherwise yields the caller's error.
    fn stale_or_error<F>(&self, key: &str, max_stale: Duration, error: F) -> ProviderResult<String>
    where
        F: FnOnce() -> ProviderError,
    {
        match self.cache.get_stale(key, max_stale) {
            Some(body) => {
                self.stats.record(Source::StaleCache);
                provider_log!("{}: serving stale cached response", self.provider);
                Ok(body)
            }
            None => Err(error()),
        }
    }

    /// Waits until this client is allowed to send another request.
    async fn wait_for_slot(&self) {
        let mut gate = self.gate.lock().await;

        if gate.window_start.elapsed() >= Duration::from_secs(60) {
            gate.window_start = Instant::now();
            gate.window_count = 0;
        }

        if gate.window_count >= self.budget_per_minute {
            let remaining = Duration::from_secs(60).saturating_sub(gate.window_start.elapsed());
            provider_log!(
                "{}: per-minute budget of {} reached, waiting {} ms",
                self.provider,
                self.budget_per_minute,
                remaining.as_millis()
            );
            tokio::time::sleep(remaining).await;
            gate.window_start = Instant::now();
            gate.window_count = 0;
        }

        if let Some(last) = gate.last_request {
            let elapsed = last.elapsed();
            if elapsed < self.min_gap {
                tokio::time::sleep(self.min_gap - elapsed).await;
            }
        }

        gate.last_request = Some(Instant::now());
        gate.window_count += 1;
    }
}

/// Just the path, so logs stay one line long.
fn short_url(url: &str) -> String {
    url.rsplit("//").next().unwrap_or(url).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(cache: ResponseCache) -> HttpClient {
        HttpClient::with_policy("test", cache, Duration::from_millis(50), 3).unwrap()
    }

    fn temp_cache(tag: &str) -> ResponseCache {
        let dir = std::env::temp_dir().join(format!("rustybui-http-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        ResponseCache::at(dir)
    }

    #[tokio::test]
    async fn offline_mode_never_sends_and_uses_stale_cache() {
        let cache = temp_cache("offline");
        let key = request_key("https://offline.test/x", &[]);
        // Store an entry that is already past the freshness window.
        let entry = crate::cache::CachedResponse {
            url: "https://offline.test/x".to_string(),
            fetched_at: chrono::Utc::now().timestamp() - 400,
            body: "cached body".to_string(),
        };
        std::fs::create_dir_all(cache.dir()).unwrap();
        std::fs::write(
            cache.dir().join(format!("{key}.json")),
            serde_json::to_string(&entry).unwrap(),
        )
        .unwrap();

        let offline = ResponseCache::at(cache.dir().to_path_buf()).offline();
        let client = client(offline);
        let body = client
            .get_text("https://offline.test/x", &[], None, Duration::from_secs(300))
            .await
            .expect("stale cache should be served offline");
        assert_eq!(body, "cached body");
        assert_eq!(client.stats().snapshot(), (0, 0, 1));
    }

    #[tokio::test]
    async fn offline_mode_reports_a_missing_entry_as_unavailable() {
        let client = client(temp_cache("offline-miss").offline());
        let error = client
            .get_text("https://offline.test/missing", &[], None, Duration::from_secs(300))
            .await
            .expect_err("nothing cached, nothing to serve");
        assert_eq!(error.provider(), "test");
        assert!(matches!(error, ProviderError::Unavailable { .. }));
    }

    #[tokio::test]
    async fn fresh_cache_entries_skip_the_network() {
        let cache = temp_cache("fresh");
        let key = request_key("https://fresh.test/y", &[("a", "1".to_string())]);
        cache.put(&key, "https://fresh.test/y?a=1", "fresh body");

        let client = client(ResponseCache::at(cache.dir().to_path_buf()));
        let body = client
            .get_text("https://fresh.test/y", &[("a", "1".to_string())], None, Duration::from_secs(300))
            .await
            .unwrap();
        assert_eq!(body, "fresh body");
        assert_eq!(client.stats().snapshot(), (0, 1, 0));
    }

    #[tokio::test]
    async fn the_gate_serialises_requests_at_the_minimum_gap() {
        let client = client(temp_cache("gate"));
        let start = Instant::now();
        // Three slots fit the budget; the gate must spend 2 x 50 ms on gaps.
        for _ in 0..3 {
            client.wait_for_slot().await;
        }
        assert!(start.elapsed() >= Duration::from_millis(100));
    }
}
