//! DWD's own forecast, as Bright Sky publishes it.
//!
//! This is the German counterpart of the KNMI radar in the nowcast: same source family as
//! the radar the nowcast already prefers there, and DWD's model rather than a global one.
//! Two requests — the current hour's ten-minute readings and the hourly range the day rows
//! are aggregated from — both through the DWD client's shared gate and counters.

use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};

use super::http::HttpClient;
use super::{ProviderError, ProviderResult};
use crate::cache::ResponseCache;
use crate::forecast::Forecast;
use crate::location::Location;

const CURRENT_URL: &str = "https://api.brightsky.dev/current_weather";
const WEATHER_URL: &str = "https://api.brightsky.dev/weather";

/// DWD reissues its model hourly; 15 minutes of freshness is what Open-Meteo gets too.
const FRESH_FOR: Duration = Duration::from_secs(900);

/// How far ahead to ask for hourly records. Bright Sky is hourly only, so the day rows
/// come out of the same download, and ten days is its maximum range.
const FORECAST_DAYS: i64 = 8;

pub struct DwdForecastClient {
    http: HttpClient,
}

impl DwdForecastClient {
    /// Takes an existing client rather than building one: DWD's radar talks to the same
    /// service, and one source should have one politeness gate and one set of counters.
    pub fn new(http: HttpClient) -> Self {
        Self { http }
    }

    pub fn from_cache(cache: ResponseCache) -> ProviderResult<Self> {
        Ok(Self::new(HttpClient::new("DWD", cache)?))
    }

    /// Response counters for this client.
    pub fn stats(&self) -> super::FetchStats {
        self.http.stats()
    }

    /// Current conditions plus an hourly range for `location`.
    ///
    /// `timezone` must be an IANA zone or `UTC`: Bright Sky stamps its records in the
    /// location's own offset, and that offset is what the day rows are grouped by.
    pub async fn fetch_forecast(
        &self,
        location: &Location,
        timezone: &str,
    ) -> ProviderResult<Forecast> {
        let timezone = if timezone.is_empty() || timezone == "auto" { "UTC" } else { timezone };
        let position = vec![
            ("lat", format!("{:.4}", location.latitude)),
            ("lon", format!("{:.4}", location.longitude)),
        ];

        let today = Utc::now().date_naive();
        let range = vec![
            ("lat", format!("{:.4}", location.latitude)),
            ("lon", format!("{:.4}", location.longitude)),
            ("date", today.format("%Y-%m-%d").to_string()),
            (
                "last_date",
                (today + ChronoDuration::days(FORECAST_DAYS)).format("%Y-%m-%d").to_string(),
            ),
            ("tz", timezone.to_string()),
        ];

        let current = self.http.get_text(CURRENT_URL, &position, None, FRESH_FOR).await?;
        let weather = self.http.get_text(WEATHER_URL, &range, None, FRESH_FOR).await?;

        let forecast = Forecast::from_bright_sky(
            &current,
            &weather,
            location.latitude,
            location.longitude,
            timezone,
        )
        .map_err(|reason| ProviderError::Decode { provider: "DWD", reason })?;

        // A well-formed but empty answer (a date the service will not serve, say) is a
        // reason to try the other provider, not to hand the app a screen with nothing on it.
        if forecast.current.is_none() && forecast.hourly.is_empty() {
            return Err(ProviderError::Unavailable {
                provider: "DWD",
                reason: "the response carried no current or hourly records".to_string(),
            });
        }

        Ok(forecast)
    }
}
