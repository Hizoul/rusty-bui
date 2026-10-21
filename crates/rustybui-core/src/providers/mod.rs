//! Provider clients — only the three this port supports: KNMI, DWD (via Bright
//! Sky) and Open-Meteo.

pub mod brightsky;
pub mod dwd_radar;
pub mod http;
pub mod knmi;
pub mod openmeteo;

pub use brightsky::BrightSkyClient;
pub use dwd_radar::DwdRadarClient;
pub use http::{FetchStats, HttpClient, Source};
pub use knmi::KnmiNowcastClient;
pub use openmeteo::OpenMeteoClient;

use crate::alerts::Alert;
use crate::cache::{cycle_start, DiskCache, ResponseCache, RESPONSE_FRESH_SECONDS};
use crate::forecast::Forecast;
use crate::location::Location;
use crate::nowcast::{NowcastProvider, RainNowcast};
use crate::radar::{RadarImage, RadarMapClient};
use chrono::{DateTime, NaiveDate, Utc};
use std::time::Duration as StdDuration;

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

/// Which provider answered a forecast, so a screen can say where its numbers came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastProvider {
    Dwd,
    OpenMeteo,
}

impl ForecastProvider {
    pub fn display_name(self) -> &'static str {
        match self {
            ForecastProvider::Dwd => "DWD",
            ForecastProvider::OpenMeteo => "Open-Meteo",
        }
    }

    /// CC BY 4.0 attribution line.
    pub fn attribution(self) -> &'static str {
        match self {
            ForecastProvider::Dwd => "Forecast © DWD (Bright Sky)",
            ForecastProvider::OpenMeteo => "Forecast © Open-Meteo",
        }
    }
}

/// Picks the best available rain nowcast for a location and caches it per radar
/// cycle, the same source order WeatherMaster uses:
///
/// * inside KNMI coverage (NL): KNMI radar → DWD radar → Open-Meteo model,
/// * everywhere else: DWD radar → KNMI radar → Open-Meteo model.
///
/// The same service answers forecasts — DWD's own model inside Germany, Open-Meteo
/// everywhere else — because both read the same caches and share one gate per source.
pub struct NowcastService {
    knmi_api_key: Option<String>,
    cache: DiskCache,
    responses: ResponseCache,
    clients: ClientSet,
}

struct ClientSet {
    knmi: KnmiNowcastClient,
    dwd: DwdRadarClient,
    brightsky: BrightSkyClient,
    radar: RadarMapClient,
    openmeteo: OpenMeteoClient,
}

impl ClientSet {
    fn new(knmi_api_key: &Option<String>, responses: &ResponseCache) -> ProviderResult<Self> {
        // One DWD client for all four DWD consumers: one gate, one set of counters. The map
        // publishes the same composite the radar client samples, through a different service.
        let dwd_http = HttpClient::new("DWD", responses.clone())?;
        Ok(Self {
            knmi: KnmiNowcastClient::new(knmi_api_key.clone(), responses.clone())?,
            dwd: DwdRadarClient::with_http(dwd_http.clone()),
            brightsky: BrightSkyClient::new(dwd_http.clone()),
            radar: RadarMapClient::new(dwd_http, responses.dir().join("radar")),
            openmeteo: OpenMeteoClient::new(responses.clone())?,
        })
    }
}

/// How long one day's forecast stays worth keeping.
///
/// A day that has already happened does not change, and reopening a row in the list should not
/// ask the service again; today and the days ahead are reissued through the day.
pub fn day_freshness(date: NaiveDate, today: NaiveDate) -> StdDuration {
    if date < today {
        StdDuration::from_secs(86_400)
    } else {
        StdDuration::from_secs(RESPONSE_FRESH_SECONDS as u64)
    }
}

impl NowcastService {
    /// `knmi_api_key` is optional: anonymous KNMI access works, a personal key
    /// just moves the request to the keyed host with its own quota.
    pub fn new(knmi_api_key: Option<String>) -> ProviderResult<Self> {
        crate::cache::tighten_cache_files();
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
    /// DWD's counter covers its radar and its forecast together.
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

    /// Forecast for one day, for a daily row opened up. Same provider preference and the same
    /// fallback as [`Self::forecast`], but asked for a single date.
    pub async fn forecast_on(
        &self,
        location: &Location,
        timezone: &str,
        date: chrono::NaiveDate,
    ) -> ProviderResult<(Forecast, ForecastProvider)> {
        let freshness = day_freshness(date, Utc::now().date_naive());
        let order = if location.prefers_dwd_forecast() {
            [ForecastProvider::Dwd, ForecastProvider::OpenMeteo]
        } else {
            [ForecastProvider::OpenMeteo, ForecastProvider::Dwd]
        };

        let mut failure = None;
        for provider in order {
            let attempt = match provider {
                ForecastProvider::Dwd => {
                    self.clients.brightsky.fetch_day(location, timezone, date, freshness).await
                }
                ForecastProvider::OpenMeteo => {
                    self.clients.openmeteo.fetch_day(location, timezone, date, freshness).await
                }
            };

            match attempt {
                Ok(forecast) if !forecast.hourly.is_empty() => {
                    provider_log!(
                        "day {} for {} from {} ({} hourly samples)",
                        date,
                        location.key(),
                        provider.display_name(),
                        forecast.hourly.len()
                    );
                    return Ok((forecast, provider));
                }
                Ok(_) => continue,
                Err(error) => {
                    provider_log!("day {} from {} failed: {error}", date, provider.display_name());
                    // The first provider is the one that should have answered; its failure is
                    // the one worth reporting, not the fallback's.
                    if failure.is_none() {
                        failure = Some(error);
                    }
                }
            }
        }

        Err(failure.unwrap_or(ProviderError::Unavailable {
            provider: "forecast",
            reason: format!("no provider had {date}"),
        }))
    }

    /// Forecast for `location`, preferring the provider that reports on that place: DWD's
    /// own model inside Germany, Open-Meteo everywhere else. Whichever fails falls back to
    /// the other, and the answer says which one it was.
    pub async fn forecast(
        &self,
        location: &Location,
        timezone: &str,
    ) -> ProviderResult<(Forecast, ForecastProvider)> {
        let order = if location.prefers_dwd_forecast() {
            [ForecastProvider::Dwd, ForecastProvider::OpenMeteo]
        } else {
            [ForecastProvider::OpenMeteo, ForecastProvider::Dwd]
        };

        let mut failure = None;
        for provider in order {
            let attempt = match provider {
                ForecastProvider::Dwd => {
                    self.clients.brightsky.fetch_forecast(location, timezone).await
                }
                ForecastProvider::OpenMeteo => {
                    self.clients.openmeteo.fetch_forecast(location, timezone).await
                }
            };

            match attempt {
                Ok(forecast) if forecast.current.is_some() || !forecast.hourly.is_empty() => {
                    provider_log!(
                        "forecast for {} from {} ({} hourly samples)",
                        location.key(),
                        provider.display_name(),
                        forecast.hourly.len()
                    );
                    return Ok((forecast, provider));
                }
                Ok(_) => {
                    // The provider that should have answered, answering with nothing: that is
                    // the failure worth reporting, so it is recorded only once.
                    if failure.is_none() {
                        failure = Some(ProviderError::Unavailable {
                            provider: provider.display_name(),
                            reason: "the response carried no forecast records".to_string(),
                        })
                    }
                }
                Err(error) => {
                    provider_log!(
                        "{} forecast failed for {}: {error}",
                        provider.display_name(),
                        location.key()
                    );
                    if failure.is_none() {
                        failure = Some(error);
                    }
                }
            }
        }

        Err(failure.unwrap_or(ProviderError::Unavailable {
            provider: "forecast",
            reason: "no provider answered".to_string(),
        }))
    }

    /// The weather warnings in force for `location`, worst first.
    ///
    /// Only DWD issues these through a keyless source, and only for its own warn cells: a
    /// position outside them is answered with a 404 rather than an empty list, so places DWD
    /// does not report on are not asked at all — no request, and no "alerts failed" line on a
    /// screen in Leiden that is showing the weather perfectly well otherwise.
    pub async fn alerts(&self, location: &Location) -> ProviderResult<Vec<Alert>> {
        if !location.prefers_dwd_forecast() {
            provider_log!("DWD does not warn for {}, so no warnings are fetched", location.key());
            return Ok(Vec::new());
        }

        let alerts = self.clients.brightsky.fetch_alerts(location).await?;
        provider_log!("{} warnings in force for {}", alerts.len(), location.key());
        Ok(alerts)
    }

    /// A radar image for the map: DWD's composite, georeferenced by the bounding box it was
    /// asked for, so it can be drawn straight onto a lat/lon map.
    pub async fn radar_image(
        &self,
        bbox: [f64; 4],
        width: u32,
        height: u32,
        time: Option<DateTime<Utc>>,
    ) -> ProviderResult<RadarImage> {
        self.clients.radar.image(bbox, width, height, time).await
    }

    /// The frames the composite has, oldest first.
    pub async fn radar_frames(&self) -> ProviderResult<Vec<DateTime<Utc>>> {
        self.clients.radar.frames().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_in_the_past_is_kept_and_a_day_ahead_is_reissued() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        let past = day_freshness(NaiveDate::from_ymd_opt(2026, 10, 8).unwrap(), today);
        let now = day_freshness(today, today);

        assert_eq!(past, StdDuration::from_secs(86_400));
        assert_eq!(now, StdDuration::from_secs(RESPONSE_FRESH_SECONDS as u64));
        // A day ahead is not settled either: the model will run again before it arrives.
        assert_eq!(day_freshness(NaiveDate::from_ymd_opt(2026, 10, 18).unwrap(), today), now);
    }
}
