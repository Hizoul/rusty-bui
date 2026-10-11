//! Provider clients — only the three this port supports: KNMI, DWD (via Bright
//! Sky) and Open-Meteo.

pub mod dwd_radar;
pub mod http;
pub mod knmi;
pub mod openmeteo;

pub use dwd_radar::DwdRadarClient;
pub use http::{FetchStats, HttpClient, Source};
pub use knmi::KnmiNowcastClient;
pub use openmeteo::OpenMeteoClient;

use crate::cache::{cycle_start, DiskCache, ResponseCache};
use crate::location::Location;
use crate::nowcast::{NowcastProvider, RainNowcast};
use chrono::{DateTime, Utc};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("request to {provider} failed: {source}")]
    Http {
        provider: &'static str,
        #[source]
        source: reqwest::Error,
    },
    #[error("{provider} returned an unusable response: {reason}")]
    Decode { provider: &'static str, reason: String },
    #[error("{provider} has no data: {reason}")]
    Unavailable { provider: &'static str, reason: String },
}

impl ProviderError {
    pub fn provider(&self) -> &'static str {
        match self {
            ProviderError::Http { provider, .. }
            | ProviderError::Decode { provider, .. }
            | ProviderError::Unavailable { provider, .. } => provider,
        }
    }
}

pub type ProviderResult<T> = std::result::Result<T, ProviderError>;

/// Fallback diagnostics, enabled with `RUSTYBUI_DEBUG=1`. A library should not
/// print on its own, but a desktop weather tool that silently falls back to a
/// coarser provider is worse.
macro_rules! provider_log {
    ($($arg:tt)*) => {
        if std::env::var_os("RUSTYBUI_DEBUG").is_some() {
            eprintln!("rustybui: {}", format!($($arg)*));
        }
    };
}
pub(crate) use provider_log;

/// Picks the best available rain nowcast for a location and caches it per radar
/// cycle, the same source order WeatherMaster uses:
///
/// * inside KNMI coverage (NL): KNMI radar → DWD radar → Open-Meteo model,
/// * everywhere else: DWD radar → KNMI radar → Open-Meteo model.
pub struct NowcastService {
    knmi_api_key: Option<String>,
    cache: DiskCache,
    responses: ResponseCache,
    clients: ClientSet,
}

struct ClientSet {
    knmi: KnmiNowcastClient,
    dwd: DwdRadarClient,
    openmeteo: OpenMeteoClient,
}

impl ClientSet {
    fn new(knmi_api_key: &Option<String>, responses: &ResponseCache) -> ProviderResult<Self> {
        Ok(Self {
            knmi: KnmiNowcastClient::new(knmi_api_key.clone(), responses.clone())?,
            dwd: DwdRadarClient::new(responses.clone())?,
            openmeteo: OpenMeteoClient::new(responses.clone())?,
        })
    }
}

impl NowcastService {
    /// `knmi_api_key` is optional: anonymous KNMI access works, a personal key
    /// just moves the request to the keyed host with its own quota.
    pub fn new(knmi_api_key: Option<String>) -> ProviderResult<Self> {
        let responses = ResponseCache::new();
        let clients = ClientSet::new(&knmi_api_key, &responses)?;
        Ok(Self { knmi_api_key, cache: DiskCache::new(), responses, clients })
    }

    /// Swap the raw-response cache, e.g. for a test directory.
    pub fn with_response_cache(mut self, responses: ResponseCache) -> ProviderResult<Self> {
        self.clients = ClientSet::new(&self.knmi_api_key, &responses)?;
        self.responses = responses;
        Ok(self)
    }

    /// Serve every provider answer from disk and never touch the network.
    pub fn offline(self, offline: bool) -> ProviderResult<Self> {
        if !offline {
            return Ok(self);
        }
        let responses = self.responses.clone().offline();
        self.with_response_cache(responses)
    }

    pub fn with_cache(mut self, cache: DiskCache) -> Self {
        self.cache = cache;
        self
    }

    pub fn cache(&self) -> &DiskCache {
        &self.cache
    }

    pub fn response_cache(&self) -> &ResponseCache {
        &self.responses
    }

    /// `(KNMI, DWD, Open-Meteo)` response counters — `(network, cache, stale)`.
    pub fn stats(&self) -> (FetchStats, FetchStats, FetchStats) {
        (
            self.clients.knmi.stats(),
            self.clients.dwd.stats(),
            self.clients.openmeteo.stats(),
        )
    }

    /// Cached nowcast for `location`; `force` refetches even inside a cycle.
    pub async fn nowcast(&self, location: &Location, force: bool) -> Option<RainNowcast> {
        let now = Utc::now();
        let cycle = cycle_start(now);

        if !force {
            if let Some(cached) = self.cache.load_nowcast(location, cycle) {
                provider_log!("nowcast cache hit for {} (cycle {cycle})", location.key());
                return Some(cached);
            }
        }

        let fetched = self.fetch(location, now).await;
        if let Some(nowcast) = &fetched {
            self.cache.store_nowcast(location, cycle, nowcast);
        }
        fetched
    }

    /// Runs the provider chain, first success wins.
    pub async fn fetch(&self, location: &Location, now: DateTime<Utc>) -> Option<RainNowcast> {
        let order = if location.in_knmi_coverage() {
            [NowcastProvider::Knmi, NowcastProvider::Dwd, NowcastProvider::OpenMeteo]
        } else {
            [NowcastProvider::Dwd, NowcastProvider::Knmi, NowcastProvider::OpenMeteo]
        };

        for provider in order {
            let attempt = match provider {
                NowcastProvider::Knmi => self.clients.knmi.fetch(location, now).await,
                NowcastProvider::Dwd => self.clients.dwd.fetch(location, now).await,
                NowcastProvider::OpenMeteo => self.clients.openmeteo.fetch_minutely15(location, now).await,
            };

            match attempt {
                Ok(nowcast) if !nowcast.is_empty() => {
                    provider_log!(
                        "nowcast for {} from {} ({} samples)",
                        location.key(),
                        provider.display_name(),
                        nowcast.points.len()
                    );
                    return Some(nowcast);
                }
                Ok(_) => provider_log!(
                    "{} had no usable samples for {}",
                    provider.display_name(),
                    location.key()
                ),
                Err(error) => provider_log!(
                    "{} failed for {}: {error}",
                    provider.display_name(),
                    location.key()
                ),
            }
        }

        None
    }
}
