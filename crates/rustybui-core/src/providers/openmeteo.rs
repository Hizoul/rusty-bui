//! Open-Meteo: the keyless worldwide provider. Forecast (current/hourly/daily),
//! `minutely_15` precipitation for the nowcast fallback, air quality, and
//! geocoding for location search.

use super::http::HttpClient;
use super::{ProviderError, ProviderResult};
use crate::cache::{ResponseCache, RESPONSE_FRESH_SECONDS};
use crate::forecast::Forecast;
use crate::location::Location;
use crate::nowcast::{
    NowcastPoint, NowcastProvider, RainNowcast, MODEL_SLOT_MINUTES,
    NOWCAST_PAST_CONTEXT_MINUTES, NOWCAST_WINDOW_MINUTES,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::time::Duration as StdDuration;

const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const AIR_QUALITY_URL: &str = "https://air-quality-api.open-meteo.com/v1/air-quality";

const HOURLY_FIELDS: &str = "temperature_2m,relative_humidity_2m,dew_point_2m,apparent_temperature,\
precipitation_probability,rain,showers,snowfall,weather_code,visibility,uv_index,wind_speed_10m,\
wind_direction_10m,pressure_msl";

const CURRENT_FIELDS: &str = "rain,showers,snowfall,precipitation,weather_code,wind_speed_10m,\
wind_direction_10m,wind_gusts_10m,pressure_msl,relative_humidity_2m,temperature_2m,is_day,\
apparent_temperature,uv_index,cloud_cover";

const DAILY_FIELDS: &str = "weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,\
daylight_duration,uv_index_max,rain_sum,showers_sum,snowfall_sum,precipitation_hours,\
precipitation_probability_max,wind_speed_10m_max,wind_direction_10m_dominant,dew_point_2m_mean,\
pressure_msl_mean,relative_humidity_2m_mean,visibility_min,wind_speed_10m_mean";

const MODELS: &str = "best_match";

pub struct OpenMeteoClient {
    http: HttpClient,
}

impl OpenMeteoClient {
    pub fn new(cache: ResponseCache) -> ProviderResult<Self> {
        Ok(Self { http: HttpClient::new("Open-Meteo", cache)? })
    }

    /// Response counters for this client.
    pub fn stats(&self) -> super::FetchStats {
        self.http.stats()
    }

    /// One day of this location's forecast, from the same request narrowed to a single date.
    ///
    /// `fresh_for` is the caller's: a day that has already happened will not change, and
    /// shouldn't be re-fetched every time a row is reopened, while today's is reissued hourly.
    pub async fn fetch_day(
        &self,
        location: &Location,
        timezone: &str,
        date: chrono::NaiveDate,
        fresh_for: StdDuration,
    ) -> ProviderResult<Forecast> {
        let day = date.format("%Y-%m-%d").to_string();
        let query = self
            .forecast_query(location, timezone)
            .into_iter()
            // `current` is about now, not about a date in the list.
            .filter(|(key, _)| *key != "current")
            .chain([
                ("start_date", day.clone()),
                ("end_date", day.clone()),
            ])
            .collect::<Vec<(&str, String)>>();

        let raw = self.http.get_text(FORECAST_URL, &query, None, fresh_for).await?;
        Forecast::from_open_meteo(&raw).map_err(|reason| ProviderError::Decode {
            provider: "Open-Meteo",
            reason,
        })
    }

    /// Full forecast for a location, in the location's own timezone.
    pub async fn fetch_forecast(
        &self,
        location: &Location,
        timezone: &str,
    ) -> ProviderResult<Forecast> {
        let query = self.forecast_query(location, timezone);

        // Model forecasts are reissued hourly; 15 minutes of freshness costs
        // nothing and keeps repeated app launches off the network.
        let raw = self
            .http
            .get_text(FORECAST_URL, &query, None, StdDuration::from_secs(900))
            .await?;

        Forecast::from_open_meteo(&raw).map_err(|reason| ProviderError::Decode {
            provider: "Open-Meteo",
            reason,
        })
    }

    /// The forecast request, without the parts that make one call differ from another.
    fn forecast_query(&self, location: &Location, timezone: &str) -> Vec<(&'static str, String)> {
        vec![
            ("latitude", location.latitude.to_string()),
            ("longitude", location.longitude.to_string()),
            ("timezone", timezone.to_string()),
            ("current", CURRENT_FIELDS.to_string()),
            ("hourly", HOURLY_FIELDS.to_string()),
            ("daily", DAILY_FIELDS.to_string()),
            ("timeformat", "unixtime".to_string()),
            ("models", MODELS.to_string()),
        ]
    }

    /// Keyless 15-minute model precipitation, aligned to the same ±window as the
    /// radar sources. Lower fidelity than radar, so the UI draws 15-minute bars.
    pub async fn fetch_minutely15(
        &self,
        location: &Location,
        now: DateTime<Utc>,
    ) -> ProviderResult<RainNowcast> {
        let query = vec![
            ("latitude", location.latitude.to_string()),
            ("longitude", location.longitude.to_string()),
            ("minutely_15", "precipitation".to_string()),
            ("forecast_days", "1".to_string()),
            ("timeformat", "unixtime".to_string()),
            ("models", MODELS.to_string()),
        ];

        let body = self
            .http
            .get_text(
                FORECAST_URL,
                &query,
                None,
                StdDuration::from_secs(RESPONSE_FRESH_SECONDS as u64),
            )
            .await?;

        let body: Minutely15Response = serde_json::from_str(&body).map_err(|error| {
            ProviderError::Decode {
                provider: "Open-Meteo",
                reason: format!("invalid minutely_15 JSON: {error}"),
            }
        })?;

        let block = body.minutely_15.ok_or_else(|| ProviderError::Decode {
            provider: "Open-Meteo",
            reason: "response has no minutely_15 block".to_string(),
        })?;
        let times = block.time.unwrap_or_default();
        let values = block.precipitation.unwrap_or_default();

        // `minutely_15` values are mm accumulated over each 15-minute interval;
        // the nowcast pipeline works in mm/h.
        let window_start = now - Duration::minutes(NOWCAST_PAST_CONTEXT_MINUTES);
        let window_end = now + Duration::minutes(NOWCAST_WINDOW_MINUTES);

        let mut points = Vec::new();
        for (index, seconds) in times.iter().enumerate() {
            let Some(time) = DateTime::from_timestamp(*seconds, 0) else {
                continue;
            };
            let Some(value) = values.get(index).copied().flatten() else {
                continue;
            };
            let interval_end = time + Duration::minutes(i64::from(MODEL_SLOT_MINUTES));
            if interval_end <= window_start || time >= window_end {
                continue;
            }
            points.push(NowcastPoint {
                time,
                intensity_mm_per_hour: value.max(0.0) * 4.0,
                interval_minutes: MODEL_SLOT_MINUTES,
            });
        }

        if points.is_empty() {
            return Err(ProviderError::Unavailable {
                provider: "Open-Meteo",
                reason: "no minutely_15 sample fell inside the window".to_string(),
            });
        }

        Ok(RainNowcast::new(points, NowcastProvider::OpenMeteo, false))
    }

    /// Air quality (European AQI + PM values) for the air-quality block.
    pub async fn fetch_air_quality(&self, location: &Location) -> ProviderResult<AirQuality> {
        let query = vec![
            ("latitude", location.latitude.to_string()),
            ("longitude", location.longitude.to_string()),
            (
                "current",
                "european_aqi,us_aqi,pm10,pm2_5,carbon_monoxide,nitrogen_dioxide,ozone,\
sulphur_dioxide"
                    .to_string(),
            ),
            ("timeformat", "unixtime".to_string()),
        ];

        // Air quality is published hourly.
        let body = self
            .http
            .get_text(AIR_QUALITY_URL, &query, None, StdDuration::from_secs(1800))
            .await?;

        let body: AirQualityResponse = serde_json::from_str(&body).map_err(|error| {
            ProviderError::Decode {
                provider: "Open-Meteo",
                reason: format!("invalid air-quality JSON: {error}"),
            }
        })?;

        let current = body.current.ok_or_else(|| ProviderError::Decode {
            provider: "Open-Meteo",
            reason: "air quality response has no current block".to_string(),
        })?;

        Ok(AirQuality {
            time: current.time.and_then(|t| DateTime::from_timestamp(t, 0)),
            european_aqi: current.european_aqi,
            us_aqi: current.us_aqi,
            pm10_ug_m3: current.pm10,
            pm2_5_ug_m3: current.pm2_5,
            nitrogen_dioxide_ug_m3: current.nitrogen_dioxide,
            ozone_ug_m3: current.ozone,
            sulphur_dioxide_ug_m3: current.sulphur_dioxide,
            carbon_monoxide_ug_m3: current.carbon_monoxide,
        })
    }

    /// Location search, used by the app's location picker.
    pub async fn search_places(
        &self,
        query: &str,
        count: usize,
    ) -> ProviderResult<Vec<Location>> {
        let params = vec![
            ("name", query.to_string()),
            ("count", count.to_string()),
            ("language", "en".to_string()),
            ("format", "json".to_string()),
        ];

        // Geocoding results do not change; a day of freshness is conservative.
        let body = self
            .http
            .get_text(GEOCODING_URL, &params, None, StdDuration::from_secs(86_400))
            .await?;

        let body: GeocodingResponse = serde_json::from_str(&body).map_err(|error| {
            ProviderError::Decode {
                provider: "Open-Meteo",
                reason: format!("invalid geocoding JSON: {error}"),
            }
        })?;

        Ok(body
            .results
            .unwrap_or_default()
            .into_iter()
            .map(|place| {
                // Most useful short label: city, then the first-level region.
                let label = match (&place.admin1, &place.country) {
                    (Some(admin), _) => format!("{}, {}", place.name, admin),
                    (None, Some(country)) => format!("{}, {}", place.name, country),
                    (None, None) => place.name.clone(),
                };
                let mut location = Location::new(label, place.latitude, place.longitude);
                location.country_code = place.country_code.map(|code| code.to_uppercase());
                location.timezone = place.timezone;
                location
            })
            .collect())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AirQuality {
    pub time: Option<DateTime<Utc>>,
    pub european_aqi: Option<f64>,
    pub us_aqi: Option<f64>,
    pub pm10_ug_m3: Option<f64>,
    pub pm2_5_ug_m3: Option<f64>,
    pub nitrogen_dioxide_ug_m3: Option<f64>,
    pub ozone_ug_m3: Option<f64>,
    pub sulphur_dioxide_ug_m3: Option<f64>,
    pub carbon_monoxide_ug_m3: Option<f64>,
}

impl AirQuality {
    /// Open-Meteo's European AQI bands.
    pub fn band(&self) -> &'static str {
        match self.european_aqi {
            None => "unknown",
            Some(aqi) if aqi <= 20.0 => "good",
            Some(aqi) if aqi <= 40.0 => "fair",
            Some(aqi) if aqi <= 60.0 => "moderate",
            Some(aqi) if aqi <= 80.0 => "poor",
            Some(aqi) if aqi <= 100.0 => "very poor",
            Some(_) => "extremely poor",
        }
    }
}

#[derive(Debug, Deserialize)]
struct Minutely15Response {
    minutely_15: Option<Minutely15Block>,
}

#[derive(Debug, Deserialize)]
struct Minutely15Block {
    time: Option<Vec<i64>>,
    precipitation: Option<Vec<Option<f64>>>,
}

#[derive(Debug, Deserialize)]
struct AirQualityResponse {
    current: Option<AirQualityCurrent>,
}

#[derive(Debug, Deserialize)]
struct AirQualityCurrent {
    time: Option<i64>,
    european_aqi: Option<f64>,
    us_aqi: Option<f64>,
    pm10: Option<f64>,
    pm2_5: Option<f64>,
    carbon_monoxide: Option<f64>,
    nitrogen_dioxide: Option<f64>,
    ozone: Option<f64>,
    sulphur_dioxide: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct GeocodingResponse {
    results: Option<Vec<GeocodingPlace>>,
}

#[derive(Debug, Deserialize)]
struct GeocodingPlace {
    name: String,
    latitude: f64,
    longitude: f64,
    country_code: Option<String>,
    country: Option<String>,
    admin1: Option<String>,
    timezone: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn minutely15_values_convert_to_mm_per_hour() {
        let body = r#"{"minutely_15":{"time":[1760191200,1760192100,1760193000],
            "precipitation":[0.2,null,0.05]}}"#;
        let parsed: Minutely15Response = serde_json::from_str(body).unwrap();
        let block = parsed.minutely_15.unwrap();
        let values = block.precipitation.unwrap();
        assert_eq!(values, vec![Some(0.2), None, Some(0.05)]);
        // 0.2 mm per 15 min = 0.8 mm/h
        assert!((values[0].unwrap() * 4.0 - 0.8).abs() < 1e-9);
    }

    #[test]
    fn air_quality_bands_follow_open_meteo() {
        let aqi = |value: f64| AirQuality {
            time: Some(Utc.with_ymd_and_hms(2026, 10, 11, 15, 0, 0).unwrap()),
            european_aqi: Some(value),
            us_aqi: None,
            pm10_ug_m3: None,
            pm2_5_ug_m3: None,
            nitrogen_dioxide_ug_m3: None,
            ozone_ug_m3: None,
            sulphur_dioxide_ug_m3: None,
            carbon_monoxide_ug_m3: None,
        };
        assert_eq!(aqi(10.0).band(), "good");
        assert_eq!(aqi(35.0).band(), "fair");
        assert_eq!(aqi(55.0).band(), "moderate");
        assert_eq!(aqi(75.0).band(), "poor");
        assert_eq!(aqi(95.0).band(), "very poor");
        assert_eq!(aqi(120.0).band(), "extremely poor");
    }
}
