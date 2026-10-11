//! Normalised forecast model, mapped from Open-Meteo's response and shaped so it
//! can serve both the app's blocks and (later) a Bright Sky mapping for DWD.
//!
//! Every value is optional: a provider that cannot fill a field leaves it out
//! rather than inventing one.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurrentWeather {
    pub time: Option<DateTime<Utc>>,
    pub temperature_c: Option<f64>,
    pub apparent_temperature_c: Option<f64>,
    pub relative_humidity_pct: Option<f64>,
    pub dew_point_c: Option<f64>,
    pub pressure_msl_hpa: Option<f64>,
    pub cloud_cover_pct: Option<f64>,
    pub uv_index: Option<f64>,
    pub is_day: Option<bool>,
    /// Rain + showers, mm, for the current instant.
    pub precipitation_mm: Option<f64>,
    pub snowfall_cm: Option<f64>,
    pub weather_code: Option<u32>,
    pub wind_speed_kmh: Option<f64>,
    pub wind_gusts_kmh: Option<f64>,
    pub wind_direction_deg: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HourlyForecast {
    pub time: DateTime<Utc>,
    pub temperature_c: Option<f64>,
    pub apparent_temperature_c: Option<f64>,
    pub relative_humidity_pct: Option<f64>,
    pub dew_point_c: Option<f64>,
    pub pressure_msl_hpa: Option<f64>,
    /// Rain + showers over the hour, in mm.
    pub precipitation_mm: Option<f64>,
    pub snowfall_cm: Option<f64>,
    pub precipitation_probability_pct: Option<f64>,
    pub weather_code: Option<u32>,
    pub visibility_m: Option<f64>,
    pub uv_index: Option<f64>,
    pub wind_speed_kmh: Option<f64>,
    pub wind_direction_deg: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyForecast {
    pub time: DateTime<Utc>,
    /// Local day, `YYYY-MM-DD`, as the provider reports it.
    pub date: String,
    pub temperature_max_c: Option<f64>,
    pub temperature_min_c: Option<f64>,
    pub sunrise: Option<DateTime<Utc>>,
    pub sunset: Option<DateTime<Utc>>,
    pub daylight_seconds: Option<f64>,
    pub uv_index_max: Option<f64>,
    /// Rain + showers for the day, in mm.
    pub precipitation_mm: Option<f64>,
    pub precipitation_hours: Option<f64>,
    pub precipitation_probability_max_pct: Option<f64>,
    pub snowfall_cm: Option<f64>,
    pub wind_speed_max_kmh: Option<f64>,
    pub wind_speed_mean_kmh: Option<f64>,
    pub wind_direction_dominant_deg: Option<f64>,
    pub dew_point_mean_c: Option<f64>,
    pub pressure_msl_mean_hpa: Option<f64>,
    pub relative_humidity_mean_pct: Option<f64>,
    pub visibility_min_m: Option<f64>,
    pub weather_code: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Forecast {
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,
    pub utc_offset_seconds: i64,
    pub fetched_at: DateTime<Utc>,
    pub current: Option<CurrentWeather>,
    pub hourly: Vec<HourlyForecast>,
    pub daily: Vec<DailyForecast>,
}

impl Forecast {
    /// Maps the Open-Meteo `/v1/forecast` response.
    pub fn from_open_meteo(body: &str) -> Result<Self, String> {
        let raw: RawForecast =
            serde_json::from_str(body).map_err(|error| format!("invalid JSON: {error}"))?;

        let hourly = zip_hourly(raw.hourly);
        let utc_offset_seconds = raw.utc_offset_seconds.unwrap_or(0);
        let daily = zip_daily(raw.daily, utc_offset_seconds);

        Ok(Self {
            latitude: raw.latitude.unwrap_or(f64::NAN),
            longitude: raw.longitude.unwrap_or(f64::NAN),
            timezone: raw.timezone.unwrap_or_else(|| "UTC".to_string()),
            utc_offset_seconds,
            fetched_at: Utc::now(),
            current: raw.current.map(current_from_raw),
            hourly,
            daily,
        })
    }

    /// Hourly samples from `now` (rounded down to the hour) onward.
    pub fn hourly_from(&self, now: DateTime<Utc>, hours: usize) -> Vec<HourlyForecast> {
        self.hourly
            .iter()
            .filter(|sample| sample.time + chrono::Duration::hours(1) > now)
            .take(hours)
            .cloned()
            .collect()
    }

    /// Index of today's daily entry, when the arrays line up.
    pub fn today(&self) -> Option<&DailyForecast> {
        self.daily.first()
    }
}

/// Rain and showers summed; snowfall is centimetres and stays separate.
fn liquid_mm(rain: Option<f64>, showers: Option<f64>) -> Option<f64> {
    match (rain, showers) {
        (None, None) => None,
        (rain, showers) => Some(rain.unwrap_or(0.0) + showers.unwrap_or(0.0)),
    }
}

fn current_from_raw(raw: RawCurrent) -> CurrentWeather {
    CurrentWeather {
        time: raw.time.and_then(|t| DateTime::from_timestamp(t, 0)),
        temperature_c: raw.temperature_2m,
        apparent_temperature_c: raw.apparent_temperature,
        relative_humidity_pct: raw.relative_humidity_2m,
        dew_point_c: None,
        pressure_msl_hpa: raw.pressure_msl,
        cloud_cover_pct: raw.cloud_cover,
        uv_index: raw.uv_index,
        is_day: raw.is_day.map(|value| value > 0.0),
        // Prefer the provider's own total when present.
        precipitation_mm: raw.precipitation.or_else(|| liquid_mm(raw.rain, raw.showers)),
        snowfall_cm: raw.snowfall,
        weather_code: raw.weather_code.map(|code| code as u32),
        wind_speed_kmh: raw.wind_speed_10m,
        wind_gusts_kmh: raw.wind_gusts_10m,
        wind_direction_deg: raw.wind_direction_10m,
    }
}

fn zip_hourly(raw: Option<RawHourly>) -> Vec<HourlyForecast> {
    let Some(raw) = raw else { return Vec::new() };
    let times = raw.time.unwrap_or_default();

    let at = |values: &Option<Vec<Option<f64>>>, index: usize| -> Option<f64> {
        values.as_ref().and_then(|values| values.get(index).copied().flatten())
    };
    let code_at = |index: usize| -> Option<u32> {
        raw.weather_code
            .as_ref()
            .and_then(|codes| codes.get(index).copied().flatten())
            .map(|code| code as u32)
    };

    times
        .iter()
        .enumerate()
        .filter_map(|(index, seconds)| {
            let time = DateTime::from_timestamp(*seconds, 0)?;
            Some(HourlyForecast {
                time,
                temperature_c: at(&raw.temperature_2m, index),
                apparent_temperature_c: at(&raw.apparent_temperature, index),
                relative_humidity_pct: at(&raw.relative_humidity_2m, index),
                dew_point_c: at(&raw.dew_point_2m, index),
                pressure_msl_hpa: at(&raw.pressure_msl, index),
                precipitation_mm: liquid_mm(at(&raw.rain, index), at(&raw.showers, index)),
                snowfall_cm: at(&raw.snowfall, index),
                precipitation_probability_pct: at(&raw.precipitation_probability, index),
                weather_code: code_at(index),
                visibility_m: at(&raw.visibility, index),
                uv_index: at(&raw.uv_index, index),
                wind_speed_kmh: at(&raw.wind_speed_10m, index),
                wind_direction_deg: at(&raw.wind_direction_10m, index),
            })
        })
        .collect()
}

fn zip_daily(raw: Option<RawDaily>, utc_offset_seconds: i64) -> Vec<DailyForecast> {
    let Some(raw) = raw else { return Vec::new() };
    let times = raw.time.clone().unwrap_or_default();

    let at = |values: &Option<Vec<Option<f64>>>, index: usize| -> Option<f64> {
        values.as_ref().and_then(|values| values.get(index).copied().flatten())
    };
    let seconds_at = |values: &Option<Vec<Option<i64>>>, index: usize| -> Option<DateTime<Utc>> {
        values
            .as_ref()
            .and_then(|values| values.get(index).copied().flatten())
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
    };
    let local_dates: Vec<String> = times
        .iter()
        .map(|seconds| {
            (DateTime::from_timestamp(*seconds, 0).unwrap_or_else(Utc::now)
                + chrono::Duration::seconds(utc_offset_seconds))
            .format("%Y-%m-%d")
            .to_string()
        })
        .collect();

    times
        .iter()
        .enumerate()
        .filter_map(|(index, seconds)| {
            let time = DateTime::from_timestamp(*seconds, 0)?;
            let rain = at(&raw.rain_sum, index);
            let showers = at(&raw.showers_sum, index);
            Some(DailyForecast {
                time,
                date: local_dates.get(index).cloned().unwrap_or_default(),
                temperature_max_c: at(&raw.temperature_2m_max, index),
                temperature_min_c: at(&raw.temperature_2m_min, index),
                sunrise: seconds_at(&raw.sunrise, index),
                sunset: seconds_at(&raw.sunset, index),
                daylight_seconds: at(&raw.daylight_duration, index),
                uv_index_max: at(&raw.uv_index_max, index),
                precipitation_mm: liquid_mm(rain, showers),
                precipitation_hours: at(&raw.precipitation_hours, index),
                precipitation_probability_max_pct: at(&raw.precipitation_probability_max, index),
                snowfall_cm: at(&raw.snowfall_sum, index),
                wind_speed_max_kmh: at(&raw.wind_speed_10m_max, index),
                wind_speed_mean_kmh: at(&raw.wind_speed_10m_mean, index),
                wind_direction_dominant_deg: at(&raw.wind_direction_10m_dominant, index),
                dew_point_mean_c: at(&raw.dew_point_2m_mean, index),
                pressure_msl_mean_hpa: at(&raw.pressure_msl_mean, index),
                relative_humidity_mean_pct: at(&raw.relative_humidity_2m_mean, index),
                visibility_min_m: at(&raw.visibility_min, index),
                weather_code: raw
                    .weather_code
                    .as_ref()
                    .and_then(|codes| codes.get(index).copied().flatten())
                    .map(|code| code as u32),
            })
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct RawForecast {
    latitude: Option<f64>,
    longitude: Option<f64>,
    timezone: Option<String>,
    utc_offset_seconds: Option<i64>,
    current: Option<RawCurrent>,
    hourly: Option<RawHourly>,
    daily: Option<RawDaily>,
}

#[derive(Debug, Deserialize)]
struct RawCurrent {
    time: Option<i64>,
    temperature_2m: Option<f64>,
    apparent_temperature: Option<f64>,
    relative_humidity_2m: Option<f64>,
    pressure_msl: Option<f64>,
    cloud_cover: Option<f64>,
    uv_index: Option<f64>,
    is_day: Option<f64>,
    precipitation: Option<f64>,
    rain: Option<f64>,
    showers: Option<f64>,
    snowfall: Option<f64>,
    weather_code: Option<i64>,
    wind_speed_10m: Option<f64>,
    wind_gusts_10m: Option<f64>,
    wind_direction_10m: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RawHourly {
    time: Option<Vec<i64>>,
    temperature_2m: Option<Vec<Option<f64>>>,
    relative_humidity_2m: Option<Vec<Option<f64>>>,
    dew_point_2m: Option<Vec<Option<f64>>>,
    apparent_temperature: Option<Vec<Option<f64>>>,
    precipitation_probability: Option<Vec<Option<f64>>>,
    rain: Option<Vec<Option<f64>>>,
    showers: Option<Vec<Option<f64>>>,
    snowfall: Option<Vec<Option<f64>>>,
    weather_code: Option<Vec<Option<i64>>>,
    visibility: Option<Vec<Option<f64>>>,
    uv_index: Option<Vec<Option<f64>>>,
    wind_speed_10m: Option<Vec<Option<f64>>>,
    wind_direction_10m: Option<Vec<Option<f64>>>,
    pressure_msl: Option<Vec<Option<f64>>>,
}

#[derive(Debug, Deserialize)]
struct RawDaily {
    time: Option<Vec<i64>>,
    weather_code: Option<Vec<Option<i64>>>,
    temperature_2m_max: Option<Vec<Option<f64>>>,
    temperature_2m_min: Option<Vec<Option<f64>>>,
    sunrise: Option<Vec<Option<i64>>>,
    sunset: Option<Vec<Option<i64>>>,
    daylight_duration: Option<Vec<Option<f64>>>,
    uv_index_max: Option<Vec<Option<f64>>>,
    rain_sum: Option<Vec<Option<f64>>>,
    showers_sum: Option<Vec<Option<f64>>>,
    snowfall_sum: Option<Vec<Option<f64>>>,
    precipitation_hours: Option<Vec<Option<f64>>>,
    precipitation_probability_max: Option<Vec<Option<f64>>>,
    wind_speed_10m_max: Option<Vec<Option<f64>>>,
    wind_speed_10m_mean: Option<Vec<Option<f64>>>,
    wind_direction_10m_dominant: Option<Vec<Option<f64>>>,
    dew_point_2m_mean: Option<Vec<Option<f64>>>,
    pressure_msl_mean: Option<Vec<Option<f64>>>,
    relative_humidity_2m_mean: Option<Vec<Option<f64>>>,
    visibility_min: Option<Vec<Option<f64>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "latitude": 52.16, "longitude": 4.49, "timezone": "Europe/Amsterdam",
      "utc_offset_seconds": 7200,
      "current": {"time": 1760191200, "temperature_2m": 14.2, "apparent_temperature": 13.1,
        "relative_humidity_2m": 81, "pressure_msl": 1012.4, "cloud_cover": 92, "uv_index": 1.2,
        "is_day": 1, "precipitation": 0.3, "rain": 0.3, "showers": 0.0, "snowfall": 0.0,
        "weather_code": 61, "wind_speed_10m": 18.4, "wind_gusts_10m": 33.1,
        "wind_direction_10m": 210},
      "hourly": {"time": [1760191200, 1760194800, 1760198400],
        "temperature_2m": [14.2, 14.6, null], "relative_humidity_2m": [81, 79, 77],
        "dew_point_2m": [11.0, 11.1, 11.2], "apparent_temperature": [13.1, 13.6, 13.9],
        "precipitation_probability": [80, 60, 40], "rain": [0.3, 0.1, 0.0],
        "showers": [0.0, 0.2, 0.0], "snowfall": [0.0, 0.0, 0.0],
        "weather_code": [61, 80, 3], "visibility": [12000.0, 18000.0, 24000.0],
        "uv_index": [1.2, 1.6, 2.0], "wind_speed_10m": [18.4, 20.0, 21.5],
        "wind_direction_10m": [210, 215, 220], "pressure_msl": [1012.4, 1012.9, 1013.1]},
      "daily": {"time": [1760140800, 1760227200],
        "weather_code": [61, 3], "temperature_2m_max": [15.1, 14.0],
        "temperature_2m_min": [9.2, 8.4], "sunrise": [1760161800, 1760248260],
        "sunset": [1760201460, 1760287860], "daylight_duration": [39660.0, 39600.0],
        "uv_index_max": [2.1, 2.0], "rain_sum": [1.4, 0.0], "showers_sum": [0.2, 0.0],
        "snowfall_sum": [0.0, 0.0], "precipitation_hours": [5.0, 0.0],
        "precipitation_probability_max": [85, 20], "wind_speed_10m_max": [28.1, 24.0],
        "wind_speed_10m_mean": [18.0, 15.5], "wind_direction_10m_dominant": [205, 190],
        "dew_point_2m_mean": [10.5, 9.9], "pressure_msl_mean": [1012.0, 1015.0],
        "relative_humidity_2m_mean": [82, 74], "visibility_min": [8000.0, 15000.0]}
    }"#;

    #[test]
    fn maps_current_hourly_and_daily() {
        let forecast = Forecast::from_open_meteo(SAMPLE).unwrap();
        assert_eq!(forecast.timezone, "Europe/Amsterdam");
        assert_eq!(forecast.utc_offset_seconds, 7200);

        let current = forecast.current.as_ref().unwrap();
        assert_eq!(current.temperature_c, Some(14.2));
        assert_eq!(current.precipitation_mm, Some(0.3));
        assert_eq!(current.weather_code, Some(61));
        assert_eq!(current.is_day, Some(true));

        assert_eq!(forecast.hourly.len(), 3);
        assert_eq!(forecast.hourly[2].temperature_c, None);
        // rain + showers summed per hour: 0.1 + 0.2.
        assert!((forecast.hourly[1].precipitation_mm.unwrap() - 0.3).abs() < 1e-9);

        assert_eq!(forecast.daily.len(), 2);
        let today = forecast.today().unwrap();
        assert_eq!(today.temperature_max_c, Some(15.1));
        // rain_sum + showers_sum; snowfall stays separate, in cm.
        assert!((today.precipitation_mm.unwrap() - 1.6).abs() < 1e-9);
        assert_eq!(today.snowfall_cm, Some(0.0));
        assert_eq!(today.wind_speed_mean_kmh, Some(18.0));
        assert!(today.sunrise.is_some());
        // 1760140800 = 2025-10-11T00:00:00Z, +02:00 local offset.
        assert_eq!(today.date, "2025-10-11");
    }

    #[test]
    fn rejects_garbage_instead_of_guessing() {
        assert!(Forecast::from_open_meteo("not json").is_err());
        // A valid but empty body yields an empty forecast, not an error.
        let empty = Forecast::from_open_meteo("{}").unwrap();
        assert!(empty.hourly.is_empty() && empty.daily.is_empty() && empty.current.is_none());
    }

    #[test]
    fn hourly_from_skips_past_hours() {
        let forecast = Forecast::from_open_meteo(SAMPLE).unwrap();
        let now = DateTime::from_timestamp(1760194800, 0).unwrap();
        let next = forecast.hourly_from(now, 12);
        assert_eq!(next.len(), 2);
        assert_eq!(next[0].time, now);
    }
}
