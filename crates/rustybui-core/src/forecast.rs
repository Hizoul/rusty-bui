//! Normalised forecast model, mapped from Open-Meteo's response and from DWD's own
//! model as Bright Sky publishes it, so the rest of the port never learns which
//! provider filled it in.
//!
//! Every value is optional: a provider that cannot fill a field leaves it out
//! rather than inventing one.

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::nowcast::RAIN_THRESHOLD_MM_H;

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

    /// Maps a Bright Sky pair — `/current_weather` and `/weather` — into the same shape,
    /// for DWD's own model.
    ///
    /// Bright Sky is hourly only: there is no daily block, and DWD publishes no UV index
    /// and no precipitation probability through it, so the day rows are aggregated here
    /// and those fields stay `None` rather than being invented. Timestamps arrive with the
    /// location's own UTC offset, which is where the reported offset and each record's
    /// local day come from.
    pub fn from_bright_sky(
        current_body: &str,
        weather_body: &str,
        latitude: f64,
        longitude: f64,
        timezone: &str,
    ) -> Result<Self, String> {
        let current: RawBrightSkyCurrent = serde_json::from_str(current_body)
            .map_err(|error| format!("invalid current_weather JSON: {error}"))?;
        let weather: RawBrightSkyWeather = serde_json::from_str(weather_body)
            .map_err(|error| format!("invalid weather JSON: {error}"))?;

        let records = weather.weather.unwrap_or_default();
        let utc_offset_seconds = records
            .iter()
            .find_map(|record| local_time(record.timestamp.as_deref()))
            .map(|local| local.offset().local_minus_utc() as i64)
            .unwrap_or(0);

        let hourly: Vec<HourlyForecast> = records
            .iter()
            .filter_map(|record| {
                let local = local_time(record.timestamp.as_deref())?;
                Some(HourlyForecast {
                    time: local.with_timezone(&Utc),
                    temperature_c: record.temperature,
                    // DWD publishes neither an apparent temperature nor a UV index here.
                    apparent_temperature_c: None,
                    relative_humidity_pct: record.relative_humidity,
                    dew_point_c: record.dew_point,
                    pressure_msl_hpa: record.pressure_msl,
                    precipitation_mm: record.precipitation,
                    snowfall_cm: None,
                    precipitation_probability_pct: record.precipitation_probability,
                    weather_code: record.icon.as_deref().map(wmo_from_bright_sky_icon),
                    visibility_m: record.visibility,
                    uv_index: None,
                    wind_speed_kmh: record.wind_speed,
                    wind_direction_deg: record.wind_direction,
                })
            })
            .collect();

        let current = current.weather.map(|raw| CurrentWeather {
            time: local_time(raw.timestamp.as_deref()).map(|local| local.with_timezone(&Utc)),
            temperature_c: raw.temperature,
            apparent_temperature_c: None,
            relative_humidity_pct: raw.relative_humidity,
            dew_point_c: raw.dew_point,
            pressure_msl_hpa: raw.pressure_msl,
            cloud_cover_pct: raw.cloud_cover,
            uv_index: None,
            is_day: is_day_from_icon(raw.icon.as_deref()),
            // The nearest thing Bright Sky has to "now": precipitation in the last ten
            // minutes. Open-Meteo's equivalent current value covers fifteen, and both are
            // an amount over a recent interval rather than a rate.
            precipitation_mm: raw.precipitation_10,
            snowfall_cm: None,
            weather_code: raw.icon.as_deref().map(wmo_from_bright_sky_icon),
            wind_speed_kmh: raw.wind_speed_10,
            wind_gusts_kmh: raw.wind_gust_speed_10,
            wind_direction_deg: raw.wind_direction_10,
        });

        Ok(Self {
            latitude,
            longitude,
            timezone: timezone.to_string(),
            utc_offset_seconds,
            fetched_at: Utc::now(),
            current,
            daily: aggregate_days(&hourly, utc_offset_seconds),
            hourly,
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

#[derive(Debug, Deserialize)]
struct RawBrightSkyCurrent {
    weather: Option<RawBrightSkyCurrentWeather>,
}

#[derive(Debug, Deserialize)]
struct RawBrightSkyCurrentWeather {
    timestamp: Option<String>,
    temperature: Option<f64>,
    cloud_cover: Option<f64>,
    dew_point: Option<f64>,
    pressure_msl: Option<f64>,
    relative_humidity: Option<f64>,
    wind_speed_10: Option<f64>,
    wind_gust_speed_10: Option<f64>,
    wind_direction_10: Option<f64>,
    precipitation_10: Option<f64>,
    icon: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawBrightSkyWeather {
    weather: Option<Vec<RawBrightSkyRecord>>,
}

#[derive(Debug, Deserialize)]
struct RawBrightSkyRecord {
    timestamp: Option<String>,
    temperature: Option<f64>,
    precipitation: Option<f64>,
    pressure_msl: Option<f64>,
    wind_speed: Option<f64>,
    wind_direction: Option<f64>,
    cloud_cover: Option<f64>,
    dew_point: Option<f64>,
    relative_humidity: Option<f64>,
    visibility: Option<f64>,
    wind_gust_speed: Option<f64>,
    precipitation_probability: Option<f64>,
    icon: Option<String>,
}

/// Bright Sky stamps its records in RFC 3339 with the location's own offset.
fn local_time(value: Option<&str>) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value?).ok()
}

/// Bright Sky reports DWD's icon names; the rest of the port speaks WMO codes. An icon we
/// do not recognise reads as overcast rather than as clear, because clear is the more
/// misleading of the two guesses.
pub fn wmo_from_bright_sky_icon(icon: &str) -> u32 {
    match icon {
        "clear-day" | "clear-night" => 0,
        "partly-cloudy-day" | "partly-cloudy-night" => 2,
        "fog" => 45,
        "rain" => 61,
        // Rain and snow mixed.
        "sleet" => 68,
        "snow" => 71,
        "hail" => 96,
        "thunderstorm" => 95,
        // `cloudy`, and DWD's `wind` icon, which marks a gusty sky with no precipitation of
        // its own.
        _ => 3,
    }
}

fn is_day_from_icon(icon: Option<&str>) -> Option<bool> {
    let icon = icon?;
    if icon.ends_with("-day") {
        Some(true)
    } else if icon.ends_with("-night") {
        Some(false)
    } else {
        None
    }
}

/// How bad a condition is, so a day can report its worst hour rather than whichever hour
/// happened to come first.
fn weather_severity(code: u32) -> u8 {
    match code {
        95 | 96 | 99 => 6,
        71 | 73 | 75 | 77 | 85 | 86 => 5,
        68 => 4,
        61 | 63 | 65 | 80 | 81 | 82 => 3,
        45 | 48 => 2,
        3 => 1,
        _ => 0,
    }
}

/// Groups the hourly records into local days — the aggregation Bright Sky leaves to its
/// caller, since it has no daily endpoint.
fn aggregate_days(hourly: &[HourlyForecast], utc_offset_seconds: i64) -> Vec<DailyForecast> {
    let mut days: Vec<(NaiveDate, Vec<&HourlyForecast>)> = Vec::new();
    for sample in hourly {
        let local_date =
            (sample.time + chrono::Duration::seconds(utc_offset_seconds)).date_naive();
        match days.last_mut() {
            Some((date, bucket)) if *date == local_date => bucket.push(sample),
            _ => days.push((local_date, vec![sample])),
        }
    }

    days.into_iter()
        .map(|(date, bucket)| {
            let pick = |get: fn(&HourlyForecast) -> Option<f64>| -> Vec<f64> {
                bucket.iter().filter_map(|sample| get(sample)).collect()
            };
            let temperatures = pick(|sample| sample.temperature_c);
            let precipitation = pick(|sample| sample.precipitation_mm);
            let wind_speeds = pick(|sample| sample.wind_speed_kmh);
            let wind_directions = pick(|sample| sample.wind_direction_deg);
            let humidities = pick(|sample| sample.relative_humidity_pct);
            let pressures = pick(|sample| sample.pressure_msl_hpa);
            let dew_points = pick(|sample| sample.dew_point_c);
            let visibilities = pick(|sample| sample.visibility_m);
            let codes: Vec<u32> = bucket.iter().filter_map(|sample| sample.weather_code).collect();

            let local_midnight = date.and_hms_opt(0, 0, 0).unwrap()
                - chrono::Duration::seconds(utc_offset_seconds);

            DailyForecast {
                time: DateTime::from_naive_utc_and_offset(local_midnight, Utc),
                date: date.format("%Y-%m-%d").to_string(),
                temperature_max_c: maximum(&temperatures),
                temperature_min_c: minimum(&temperatures),
                sunrise: None,
                sunset: None,
                daylight_seconds: None,
                uv_index_max: None,
                precipitation_mm: if precipitation.is_empty() {
                    None
                } else {
                    Some(precipitation.iter().sum())
                },
                precipitation_hours: Some(
                    precipitation
                        .iter()
                        .filter(|mm| **mm >= RAIN_THRESHOLD_MM_H)
                        .count() as f64,
                ),
                precipitation_probability_max_pct: None,
                snowfall_cm: None,
                wind_speed_max_kmh: maximum(&wind_speeds),
                wind_speed_mean_kmh: mean(&wind_speeds),
                wind_direction_dominant_deg: circular_mean_degrees(&wind_directions),
                dew_point_mean_c: mean(&dew_points),
                pressure_msl_mean_hpa: mean(&pressures),
                relative_humidity_mean_pct: mean(&humidities),
                visibility_min_m: minimum(&visibilities),
                weather_code: codes
                    .iter()
                    .copied()
                    .max_by_key(|code| weather_severity(*code)),
            }
        })
        .collect()
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn maximum(values: &[f64]) -> Option<f64> {
    values.iter().copied().reduce(f64::max)
}

fn minimum(values: &[f64]) -> Option<f64> {
    values.iter().copied().reduce(f64::min)
}

/// Averages bearings as vectors, so 350° and 10° average to north instead of to 180°.
fn circular_mean_degrees(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let (mut x, mut y) = (0.0, 0.0);
    for value in values {
        let radians = value.to_radians();
        x += radians.cos();
        y += radians.sin();
    }
    let mean = y.atan2(x).to_degrees();
    Some(if mean < 0.0 { mean + 360.0 } else { mean })
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

    // ---- Bright Sky (DWD's own model)

    const BRIGHT_SKY_CURRENT: &str = r#"{"weather":{
      "timestamp":"2026-10-11T17:30:00+02:00","source_id":303711,"temperature":10.9,
      "cloud_cover":100,"condition":"dry","dew_point":7.86,"icon":"cloudy",
      "precipitation_10":0.0,"pressure_msl":1008.6,"relative_humidity":82,"visibility":60702,
      "wind_direction_10":240,"wind_gust_speed_10":10.4,"wind_speed_10":7.2}}"#;

    const BRIGHT_SKY_WEATHER: &str = r#"{"sources":[],"weather":[
      {"timestamp":"2026-10-11T23:00:00+02:00","temperature":8.4,"precipitation":0.0,
       "pressure_msl":1009.0,"wind_speed":5.0,"wind_direction":350,"cloud_cover":90,
       "dew_point":6.0,"relative_humidity":80,"visibility":50000,"wind_gust_speed":12.0,
       "precipitation_probability":null,"condition":"dry","icon":"clear-night"},
      {"timestamp":"2026-10-12T00:00:00+02:00","temperature":8.1,"precipitation":0.4,
       "pressure_msl":1008.5,"wind_speed":7.5,"wind_direction":10,"cloud_cover":100,
       "dew_point":6.2,"relative_humidity":85,"visibility":20000,"wind_gust_speed":18.0,
       "precipitation_probability":null,"condition":"rain","icon":"rain"},
      {"timestamp":"2026-10-12T01:00:00+02:00","temperature":9.6,"precipitation":0.2,
       "pressure_msl":1007.0,"wind_speed":9.0,"wind_direction":20,"cloud_cover":80,
       "dew_point":6.5,"relative_humidity":78,"visibility":30000,"wind_gust_speed":22.0,
       "precipitation_probability":null,"condition":"rain","icon":"thunderstorm"}]}"#;

    fn bright_sky() -> Forecast {
        Forecast::from_bright_sky(
            BRIGHT_SKY_CURRENT,
            BRIGHT_SKY_WEATHER,
            52.52,
            13.405,
            "Europe/Berlin",
        )
        .unwrap()
    }

    #[test]
    fn maps_bright_sky_into_the_same_model() {
        let forecast = bright_sky();
        assert_eq!(forecast.timezone, "Europe/Berlin");
        // Taken from the records' own +02:00 offset, since Bright Sky echoes no zone.
        assert_eq!(forecast.utc_offset_seconds, 7200);
        assert_eq!(forecast.latitude, 52.52);
        assert_eq!(forecast.longitude, 13.405);

        let current = forecast.current.as_ref().unwrap();
        assert_eq!(current.temperature_c, Some(10.9));
        assert_eq!(current.weather_code, Some(3));
        assert_eq!(current.wind_gusts_kmh, Some(10.4));
        // `cloudy` carries no day/night suffix, so the window has to fall back.
        assert_eq!(current.is_day, None);
        // DWD publishes neither of these through Bright Sky.
        assert_eq!(current.apparent_temperature_c, None);
        assert_eq!(current.uv_index, None);
        assert_eq!(
            current.time,
            Some(
                DateTime::parse_from_rfc3339("2026-10-11T17:30:00+02:00")
                    .unwrap()
                    .with_timezone(&Utc)
            )
        );

        assert_eq!(forecast.hourly.len(), 3);
        assert_eq!(forecast.hourly[1].weather_code, Some(61));
        assert_eq!(forecast.hourly[1].precipitation_mm, Some(0.4));
        assert_eq!(forecast.hourly[1].precipitation_probability_pct, None);
        assert_eq!(forecast.hourly[0].visibility_m, Some(50000.0));
    }

    #[test]
    fn bright_sky_hours_are_grouped_into_local_days() {
        let forecast = bright_sky();
        // 23:00 on the 11th and 00:00/01:00 on the 12th are two local days, not one.
        assert_eq!(forecast.daily.len(), 2);

        let first = &forecast.daily[0];
        assert_eq!(first.date, "2026-10-11");
        assert_eq!(first.temperature_min_c, Some(8.4));
        assert_eq!(first.temperature_max_c, Some(8.4));
        assert_eq!(first.precipitation_mm, Some(0.0));
        assert_eq!(first.precipitation_hours, Some(0.0));
        assert_eq!(first.weather_code, Some(0));
        // Local midnight, i.e. 2026-10-11T00:00+02:00.
        assert_eq!(
            first.time,
            DateTime::parse_from_rfc3339("2026-10-11T00:00:00+02:00")
                .unwrap()
                .with_timezone(&Utc)
        );

        let second = &forecast.daily[1];
        assert_eq!(second.date, "2026-10-12");
        assert_eq!(second.temperature_min_c, Some(8.1));
        assert_eq!(second.temperature_max_c, Some(9.6));
        assert!((second.precipitation_mm.unwrap() - 0.6).abs() < 1e-9);
        assert_eq!(second.precipitation_hours, Some(2.0));
        // The day reports its worst hour, not its first.
        assert_eq!(second.weather_code, Some(95));
        assert_eq!(second.wind_speed_max_kmh, Some(9.0));
        assert!((second.wind_speed_mean_kmh.unwrap() - 8.25).abs() < 1e-9);
        // The 10° and 20° hours are both in this bucket.
        assert!((second.wind_direction_dominant_deg.unwrap() - 15.0).abs() < 0.5);
        assert_eq!(second.visibility_min_m, Some(20000.0));
        assert_eq!(second.uv_index_max, None);
        assert_eq!(second.sunrise, None);
    }

    #[test]
    fn bright_sky_icons_become_wmo_codes() {
        assert_eq!(wmo_from_bright_sky_icon("clear-day"), 0);
        assert_eq!(wmo_from_bright_sky_icon("clear-night"), 0);
        assert_eq!(wmo_from_bright_sky_icon("partly-cloudy-night"), 2);
        assert_eq!(wmo_from_bright_sky_icon("cloudy"), 3);
        assert_eq!(wmo_from_bright_sky_icon("fog"), 45);
        assert_eq!(wmo_from_bright_sky_icon("rain"), 61);
        assert_eq!(wmo_from_bright_sky_icon("sleet"), 68);
        assert_eq!(wmo_from_bright_sky_icon("snow"), 71);
        assert_eq!(wmo_from_bright_sky_icon("hail"), 96);
        assert_eq!(wmo_from_bright_sky_icon("thunderstorm"), 95);
        // An icon this port has never seen reads as overcast, not as clear.
        assert_eq!(wmo_from_bright_sky_icon("something-new"), 3);
    }

    #[test]
    fn bearings_average_the_short_way_round() {
        // 350° and 10° straddle north, so their average is north, not south.
        let mean = circular_mean_degrees(&[350.0, 10.0]).unwrap();
        assert!(mean < 1.0 || mean > 359.0, "averaged to {mean}");
        assert_eq!(circular_mean_degrees(&[]), None);
        assert!((circular_mean_degrees(&[90.0, 90.0]).unwrap() - 90.0).abs() < 1e-9);
    }

    #[test]
    fn bright_sky_skips_records_it_cannot_place_in_time() {
        let weather = r#"{"weather":[
          {"timestamp":"not a timestamp","temperature":1.0},
          {"timestamp":"2026-10-12T00:00:00+02:00","temperature":2.0},
          {"temperature":3.0}]}"#;
        let forecast = Forecast::from_bright_sky(
            r#"{"weather":null}"#,
            weather,
            52.52,
            13.405,
            "Europe/Berlin",
        )
        .unwrap();

        assert_eq!(forecast.hourly.len(), 1);
        assert_eq!(forecast.hourly[0].temperature_c, Some(2.0));
        assert!(forecast.current.is_none());
        assert_eq!(forecast.daily.len(), 1);
    }

    #[test]
    fn bright_sky_reports_bad_json_instead_of_guessing() {
        assert!(Forecast::from_bright_sky("{}", "not json", 0.0, 0.0, "UTC").is_err());
        assert!(Forecast::from_bright_sky("not json", "{}", 0.0, 0.0, "UTC").is_err());
        // Valid but empty: an empty forecast, not an error.
        let empty = Forecast::from_bright_sky("{}", "{}", 0.0, 0.0, "UTC").unwrap();
        assert!(empty.hourly.is_empty() && empty.daily.is_empty() && empty.current.is_none());
    }
}
