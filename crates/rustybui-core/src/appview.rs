//! Views for the app front-end: one payload with everything a screen needs, with
//! the labels already formatted.
//!
//! Same rule as [`crate::view::NowcastView`]: Rust owns the domain, QML owns the
//! pixels. Times are formatted with the location's own UTC offset (Open-Meteo
//! reports it), so no timezone database is needed and a location in another zone
//! still reads correctly, and every instant also ships as epoch milliseconds.

use crate::alerts::Alert;
use crate::forecast::{DailyForecast, Forecast, HourlyForecast};
use crate::location::Location;
use crate::nowcast::RainNowcast;
use crate::providers::openmeteo::AirQuality;
use crate::providers::ForecastProvider;
use crate::view::{LocationView, NowcastView};
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

/// WMO weather interpretation codes, as Open-Meteo and Bright Sky both report
/// them.
pub fn condition_label(code: u32) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 => "Fog",
        48 => "Depositing rime fog",
        51 => "Light drizzle",
        53 => "Drizzle",
        55 => "Dense drizzle",
        56 => "Light freezing drizzle",
        57 => "Freezing drizzle",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 => "Light freezing rain",
        67 => "Freezing rain",
        71 => "Light snowfall",
        73 => "Snowfall",
        75 => "Heavy snowfall",
        77 => "Snow grains",
        80 => "Light rain showers",
        81 => "Rain showers",
        82 => "Violent rain showers",
        85 => "Light snow showers",
        86 => "Heavy snow showers",
        95 => "Thunderstorm",
        96 => "Thunderstorm with hail",
        99 => "Thunderstorm with heavy hail",
        _ => "Unknown",
    }
}

/// Sixteen-point compass, as weather apps print it next to a wind speed.
pub fn wind_compass(degrees: f64) -> &'static str {
    const POINTS: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    if !degrees.is_finite() {
        return "";
    }
    let index = ((degrees.rem_euclid(360.0) + 11.25) / 22.5).floor() as usize % 16;
    POINTS[index]
}

fn temperature(value: Option<f64>) -> Option<String> {
    value.map(|c| format!("{c:.1}°"))
}

fn millimetres(value: f64, imperial: bool) -> String {
    if imperial {
        format!("{:.3} in", value / 25.4)
    } else {
        format!("{value:.2} mm")
    }
}

fn speed(value: f64, imperial: bool) -> String {
    if imperial {
        format!("{:.1} mph", value * 0.621_371)
    } else {
        format!("{value:.1} km/h")
    }
}

fn clock(time: DateTime<FixedOffset>) -> String {
    time.format("%H:%M").to_string()
}

fn day(time: DateTime<FixedOffset>) -> String {
    time.format("%a %-d %b").to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentView {
    pub time_ms: Option<i64>,
    /// Observed time in the location's own offset, e.g. `16:00`.
    pub clock_label: Option<String>,
    pub temperature_c: Option<f64>,
    pub temperature_label: Option<String>,
    pub feels_like_label: Option<String>,
    pub condition_label: String,
    pub weather_code: Option<u32>,
    pub is_day: Option<bool>,
    pub wind_label: Option<String>,
    pub humidity_label: Option<String>,
    pub pressure_label: Option<String>,
    pub uv_label: Option<String>,
    pub cloud_cover_pct: Option<f64>,
    pub precipitation_mm: Option<f64>,
    pub precipitation_label: Option<String>,
    pub snowfall_cm: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourlyView {
    pub time_ms: i64,
    pub clock_label: String,
    pub temperature_c: Option<f64>,
    pub temperature_label: Option<String>,
    pub condition_label: String,
    pub weather_code: Option<u32>,
    pub precipitation_mm: Option<f64>,
    pub precipitation_label: Option<String>,
    pub precipitation_probability_pct: Option<f64>,
    pub probability_label: Option<String>,
    pub wind_label: Option<String>,
    pub is_daylight: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyView {
    pub time_ms: i64,
    pub date: String,
    pub day_label: String,
    pub high_label: Option<String>,
    pub low_label: Option<String>,
    pub high_c: Option<f64>,
    pub low_c: Option<f64>,
    pub condition_label: String,
    pub weather_code: Option<u32>,
    pub precipitation_label: Option<String>,
    pub precipitation_probability_pct: Option<f64>,
    pub uv_label: Option<String>,
    pub sunrise_label: Option<String>,
    pub sunset_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForecastView {
    pub timezone: String,
    pub utc_offset_seconds: i64,
    pub current: Option<CurrentView>,
    pub hourly: Vec<HourlyView>,
    pub daily: Vec<DailyView>,
}

impl ForecastView {
    /// `hours` hourly rows from the first sample at or after `now`.
    pub fn new(forecast: &Forecast, now: DateTime<Utc>, hours: usize, imperial: bool) -> Self {
        let offset = FixedOffset::east_opt(forecast.utc_offset_seconds as i32)
            .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());

        let hourly = forecast
            .hourly
            .iter()
            .filter(|sample| sample.time + chrono::Duration::hours(1) > now)
            .take(hours)
            .map(|sample| hourly_view(sample, offset, imperial))
            .collect();

        let daily = forecast
            .daily
            .iter()
            .map(|day_forecast| daily_view(day_forecast, offset, imperial))
            .collect();

        Self {
            timezone: forecast.timezone.clone(),
            utc_offset_seconds: forecast.utc_offset_seconds,
            current: forecast
                .current
                .as_ref()
                .map(|current| current_view(current, offset, imperial)),
            hourly,
            daily,
        }
    }

    /// Today's entry, when the provider returned one.
    pub fn today(&self) -> Option<&DailyView> {
        self.daily.first()
    }
}

fn current_view(
    current: &crate::forecast::CurrentWeather,
    offset: FixedOffset,
    imperial: bool,
) -> CurrentView {
    CurrentView {
        time_ms: current.time.map(|time| time.timestamp_millis()),
        clock_label: current
            .time
            .map(|time| clock(time.with_timezone(&offset))),
        temperature_c: current.temperature_c,
        temperature_label: temperature(current.temperature_c),
        feels_like_label: current
            .apparent_temperature_c
            .map(|value| format!("feels {value:.1}°")),
        condition_label: current
            .weather_code
            .map_or("—", condition_label)
            .to_string(),
        weather_code: current.weather_code,
        is_day: current.is_day,
        wind_label: current.wind_speed_kmh.map(|speed_kmh| {
            let compass = current
                .wind_direction_deg
                .map(wind_compass)
                .filter(|point| !point.is_empty());
            match compass {
                Some(point) => format!("{} {}", speed(speed_kmh, imperial), point),
                None => speed(speed_kmh, imperial),
            }
        }),
        humidity_label: current
            .relative_humidity_pct
            .map(|value| format!("{value:.0}%")),
        pressure_label: current
            .pressure_msl_hpa
            .map(|value| format!("{value:.0} hPa")),
        uv_label: current.uv_index.map(|value| format!("{value:.1}")),
        cloud_cover_pct: current.cloud_cover_pct,
        precipitation_mm: current.precipitation_mm,
        precipitation_label: current
            .precipitation_mm
            .filter(|value| *value > 0.0)
            .map(|value| millimetres(value, imperial)),
        snowfall_cm: current.snowfall_cm.filter(|value| *value > 0.0),
    }
}

fn hourly_view(sample: &HourlyForecast, offset: FixedOffset, imperial: bool) -> HourlyView {
    let local = sample.time.with_timezone(&offset);
    HourlyView {
        time_ms: sample.time.timestamp_millis(),
        clock_label: clock(local),
        temperature_c: sample.temperature_c,
        temperature_label: temperature(sample.temperature_c),
        condition_label: sample.weather_code.map_or("—", condition_label).to_string(),
        weather_code: sample.weather_code,
        precipitation_mm: sample.precipitation_mm,
        precipitation_label: sample
            .precipitation_mm
            .map(|value| millimetres(value, imperial)),
        precipitation_probability_pct: sample.precipitation_probability_pct,
        probability_label: sample
            .precipitation_probability_pct
            .map(|value| format!("{value:.0}%")),
        wind_label: sample.wind_speed_kmh.map(|value| speed(value, imperial)),
        is_daylight: {
            // The provider only gives sunrise/sunset per day, so use the hour.
            let hour = local.format("%H").to_string().parse::<u32>().unwrap_or(12);
            (7..19).contains(&hour)
        },
    }
}

fn daily_view(day_forecast: &DailyForecast, offset: FixedOffset, imperial: bool) -> DailyView {
    let local = day_forecast.time.with_timezone(&offset);
    DailyView {
        time_ms: day_forecast.time.timestamp_millis(),
        date: day_forecast.date.clone(),
        day_label: day(local),
        high_label: temperature(day_forecast.temperature_max_c),
        low_label: temperature(day_forecast.temperature_min_c),
        high_c: day_forecast.temperature_max_c,
        low_c: day_forecast.temperature_min_c,
        condition_label: day_forecast
            .weather_code
            .map_or("—", condition_label)
            .to_string(),
        weather_code: day_forecast.weather_code,
        precipitation_label: day_forecast
            .precipitation_mm
            .map(|value| millimetres(value, imperial)),
        precipitation_probability_pct: day_forecast.precipitation_probability_max_pct,
        uv_label: day_forecast.uv_index_max.map(|value| format!("{value:.1}")),
        sunrise_label: day_forecast
            .sunrise
            .map(|time| clock(time.with_timezone(&offset))),
        sunset_label: day_forecast
            .sunset
            .map(|time| clock(time.with_timezone(&offset))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AirQualityView {
    pub time_ms: Option<i64>,
    pub european_aqi: Option<f64>,
    pub us_aqi: Option<f64>,
    pub band: String,
    pub band_label: String,
    pub pm25_label: Option<String>,
    pub pm10_label: Option<String>,
}

impl From<&AirQuality> for AirQualityView {
    fn from(air: &AirQuality) -> Self {
        let band = air.band().to_string();
        let band_label = {
            let mut chars = band.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => band.clone(),
            }
        };

        Self {
            time_ms: air.time.map(|time| time.timestamp_millis()),
            european_aqi: air.european_aqi,
            us_aqi: air.us_aqi,
            band,
            band_label,
            pm25_label: air.pm2_5_ug_m3.map(|value| format!("{value:.1} µg/m³")),
            pm10_label: air.pm10_ug_m3.map(|value| format!("{value:.1} µg/m³")),
        }
    }
}

/// One warning, as a screen shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertView {
    pub event: String,
    pub headline: String,
    pub severity_label: String,
    /// 0–3 — minor, moderate, severe, extreme — so a screen can pick a colour band without
    /// reading English, and show the worst first.
    pub severity_rank: u8,
    /// When it applies, in the location's own time: "08:00–19:00", "until 19:00", …
    pub window_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
}

impl AlertView {
    pub fn new(alert: &Alert, offset: FixedOffset) -> Self {
        let clock = |value: Option<DateTime<Utc>>| {
            value.map(|time| time.with_timezone(&offset).format("%H:%M").to_string())
        };
        let window_label = match (clock(alert.onset), clock(alert.expires)) {
            (Some(onset), Some(expires)) => format!("{onset}–{expires}"),
            (Some(onset), None) => format!("from {onset}"),
            (None, Some(expires)) => format!("until {expires}"),
            (None, None) => "in force".to_string(),
        };

        Self {
            event: alert.event.clone(),
            headline: alert.headline.clone(),
            severity_label: alert.severity.label().to_string(),
            severity_rank: alert.severity.rank(),
            window_label,
            detail: alert.description.clone(),
            instruction: alert.instruction.clone(),
        }
    }
}

/// Everything one screen needs, in one payload and one process spawn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    pub location: LocationView,
    pub generated_at_ms: i64,
    /// Warnings in force, worst first. Empty is the normal case.
    pub alerts: Vec<AlertView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nowcast: Option<NowcastView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast: Option<ForecastView>,
    /// Who supplied the forecast — DWD's own model or Open-Meteo — for the footer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast_attribution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub air: Option<AirQualityView>,
    /// Anything that failed, so a screen can say so instead of showing gaps.
    pub errors: Vec<String>,
}

/// What the app managed to fetch; each part is optional and independent.
#[derive(Debug, Default, Clone)]
pub struct AppParts<'a> {
    pub nowcast: Option<&'a RainNowcast>,
    pub forecast: Option<&'a Forecast>,
    /// Which provider answered `forecast`, so the screen can attribute what it shows.
    pub forecast_provider: Option<ForecastProvider>,
    /// Warnings in force, worst first.
    pub alerts: &'a [Alert],
    pub air: Option<&'a AirQuality>,
    pub errors: Vec<String>,
}

impl AppView {
    pub fn build(
        location: &Location,
        now: DateTime<Utc>,
        parts: AppParts<'_>,
        hours: usize,
        imperial: bool,
    ) -> Self {
        let offset = parts
            .forecast
            .map(|forecast| forecast.utc_offset_seconds)
            .and_then(|seconds| FixedOffset::east_opt(seconds as i32))
            .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());

        Self {
            location: LocationView::from(location),
            generated_at_ms: now.timestamp_millis(),
            // The forecast carries the location's zone offset; without one, warning times
            // read in UTC rather than in an invented zone.
            alerts: parts
                .alerts
                .iter()
                .map(|alert| AlertView::new(alert, offset))
                .collect(),
            nowcast: parts
                .nowcast
                .map(|nowcast| NowcastView::new(location, nowcast, now)),
            forecast: parts
                .forecast
                .map(|forecast| ForecastView::new(forecast, now, hours, imperial)),
            forecast_attribution: parts
                .forecast_provider
                .map(|provider| provider.attribution().to_string()),
            air: parts.air.map(AirQualityView::from),
            errors: parts.errors,
        }
    }

    /// One line for a notification or a bar tooltip.
    pub fn headline(&self) -> String {
        // The nowcast sentence already leads with the location name.
        if let Some(nowcast) = &self.nowcast {
            return nowcast.sentence();
        }

        let detail = match &self.forecast {
            Some(forecast) => match &forecast.current {
                Some(current) => format!(
                    "{} · {}",
                    current.temperature_label.clone().unwrap_or_else(|| "-".to_string()),
                    current.condition_label
                ),
                None => "no current conditions".to_string(),
            },
            None => "no data".to_string(),
        };
        format!("{}: {detail}", self.location.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forecast::Forecast;
    use crate::nowcast::{NowcastPoint, NowcastProvider, RainNowcast};
    use chrono::TimeZone;

    const SAMPLE: &str = r#"{
      "latitude": 52.16, "longitude": 4.49, "timezone": "Europe/Amsterdam",
      "utc_offset_seconds": 7200,
      "current": {"time": 1760191200, "temperature_2m": 14.2, "apparent_temperature": 13.1,
        "relative_humidity_2m": 81, "pressure_msl": 1012.4, "cloud_cover": 92, "uv_index": 1.2,
        "is_day": 1, "precipitation": 0.3, "rain": 0.3, "showers": 0.0, "snowfall": 0.0,
        "weather_code": 61, "wind_speed_10m": 18.4, "wind_gusts_10m": 33.1,
        "wind_direction_10m": 210},
      "hourly": {"time": [1760191200, 1760194800],
        "temperature_2m": [14.2, 14.6], "relative_humidity_2m": [81, 79],
        "precipitation_probability": [80, 60], "rain": [0.3, 0.1], "showers": [0.0, 0.2],
        "snowfall": [0.0, 0.0], "weather_code": [61, 80], "visibility": [12000.0, 18000.0],
        "uv_index": [1.2, 1.6], "wind_speed_10m": [18.4, 20.0], "wind_direction_10m": [210, 215],
        "pressure_msl": [1012.4, 1012.9]},
      "daily": {"time": [1760140800, 1760227200],
        "weather_code": [61, 3], "temperature_2m_max": [15.1, 14.0],
        "temperature_2m_min": [9.2, 8.4], "sunrise": [1760161800, 1760248260],
        "sunset": [1760201460, 1760287860], "uv_index_max": [2.1, 2.0],
        "rain_sum": [1.4, 0.0], "showers_sum": [0.2, 0.0], "snowfall_sum": [0.0, 0.0],
        "precipitation_hours": [5.0, 0.0], "precipitation_probability_max": [85, 20],
        "wind_speed_10m_max": [28.1, 24.0]}
    }"#;

    fn forecast() -> Forecast {
        Forecast::from_open_meteo(SAMPLE).unwrap()
    }

    #[test]
    fn wmo_codes_read_as_words() {
        assert_eq!(condition_label(0), "Clear");
        assert_eq!(condition_label(3), "Overcast");
        assert_eq!(condition_label(61), "Light rain");
        assert_eq!(condition_label(82), "Violent rain showers");
        assert_eq!(condition_label(95), "Thunderstorm");
        assert_eq!(condition_label(1234), "Unknown");
    }

    #[test]
    fn wind_reads_as_a_compass_point() {
        assert_eq!(wind_compass(0.0), "N");
        assert_eq!(wind_compass(90.0), "E");
        assert_eq!(wind_compass(210.0), "SSW");
        assert_eq!(wind_compass(359.0), "N");
        assert_eq!(wind_compass(f64::NAN), "");
        // Wrapping, not panicking, on out-of-range input: 450 is due east again,
        // 725 wraps to 5 degrees, which is still north.
        assert_eq!(wind_compass(450.0), "E");
        assert_eq!(wind_compass(725.0), "N");
    }

    #[test]
    fn current_block_carries_labels_not_raw_numbers_alone() {
        let view = ForecastView::new(
            &forecast(),
            Utc.with_ymd_and_hms(2025, 10, 11, 15, 0, 0).unwrap(),
            12,
            false,
        );
        let current = view.current.as_ref().expect("current");

        assert_eq!(current.temperature_label.as_deref(), Some("14.2°"));
        // 1760191200 = 2025-10-11T14:00:00Z = 16:00 in the location's +02:00.
        assert_eq!(current.clock_label.as_deref(), Some("16:00"));
        assert_eq!(current.feels_like_label.as_deref(), Some("feels 13.1°"));
        assert_eq!(current.condition_label, "Light rain");
        assert_eq!(current.wind_label.as_deref(), Some("18.4 km/h SSW"));
        assert_eq!(current.humidity_label.as_deref(), Some("81%"));
        assert_eq!(current.pressure_label.as_deref(), Some("1012 hPa"));
        assert_eq!(current.precipitation_label.as_deref(), Some("0.30 mm"));
        assert_eq!(current.is_day, Some(true));

        // Imperial, when the config asks for it.
        let imperial = ForecastView::new(
            &forecast(),
            Utc.with_ymd_and_hms(2025, 10, 11, 15, 0, 0).unwrap(),
            12,
            true,
        );
        let current = imperial.current.as_ref().unwrap();
        assert_eq!(current.wind_label.as_deref(), Some("11.4 mph SSW"));
    }

    #[test]
    fn hourly_and_daily_rows_are_clocked_in_the_locations_own_offset() {
        let now = Utc.with_ymd_and_hms(2025, 10, 11, 15, 0, 0).unwrap();
        let view = ForecastView::new(&forecast(), now, 12, false);

        // 1760191200 = 2025-10-11T14:00:00Z, which the +02:00 offset reads as 16:00,
        // and the window starts at 15:00Z — so only the later sample survives.
        assert_eq!(view.hourly.len(), 1);
        assert_eq!(view.hourly[0].clock_label, "17:00");
        assert_eq!(view.hourly[0].temperature_label.as_deref(), Some("14.6°"));
        assert_eq!(view.hourly[0].condition_label, "Light rain showers");
        assert_eq!(view.hourly[0].probability_label.as_deref(), Some("60%"));

        let today = view.today().expect("today");
        assert_eq!(today.date, "2025-10-11");
        assert_eq!(today.day_label, "Sat 11 Oct");
        assert_eq!(today.high_label.as_deref(), Some("15.1°"));
        assert_eq!(today.low_label.as_deref(), Some("9.2°"));
        assert_eq!(today.condition_label, "Light rain");
        // 1760201460 = 2025-10-11T16:51:00Z = 18:51 local.
        assert_eq!(today.sunset_label.as_deref(), Some("18:51"));
    }

    #[test]
    fn app_view_round_trips_and_survives_missing_parts() {
        let now = Utc.with_ymd_and_hms(2025, 10, 11, 15, 0, 0).unwrap();
        let nowcast = RainNowcast::new(
            vec![NowcastPoint {
                time: now,
                intensity_mm_per_hour: 0.4,
                interval_minutes: 5,
            }],
            NowcastProvider::Knmi,
            true,
        );
        let air = AirQuality {
            time: Some(now),
            european_aqi: Some(23.0),
            us_aqi: Some(30.0),
            pm10_ug_m3: Some(9.1),
            pm2_5_ug_m3: Some(3.3),
            nitrogen_dioxide_ug_m3: None,
            ozone_ug_m3: None,
            sulphur_dioxide_ug_m3: None,
            carbon_monoxide_ug_m3: None,
        };

        let view = AppView::build(
            &Location::new("Leiden", 52.16, 4.49),
            now,
            AppParts {
                nowcast: Some(&nowcast),
                forecast: Some(&forecast()),
                forecast_provider: Some(ForecastProvider::Dwd),
                alerts: &[],
                air: Some(&air),
                errors: vec!["forecast: stale".to_string()],
            },
            12,
            false,
        );

        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["location"]["name"], "Leiden");
        assert_eq!(json["nowcast"]["summary"]["pill"], "0.40 mm/h");
        assert_eq!(json["forecast"]["current"]["conditionLabel"], "Light rain");
        assert_eq!(json["air"]["band"], "fair");
        assert_eq!(json["air"]["bandLabel"], "Fair");
        // camelCase collapses the underscores: pm25Label, next to pm10Label.
        assert_eq!(json["air"]["pm25Label"], "3.3 µg/m³");
        assert_eq!(json["errors"][0], "forecast: stale");
        assert!(json["generatedAtMs"].is_i64());

        // A screen with nothing but a location still serialises.
        let empty = AppView::build(
            &Location::new("Nowhere", 0.0, 0.0),
            now,
            AppParts { errors: vec!["no coverage".to_string()], ..Default::default() },
            12,
            false,
        );
        let json = serde_json::to_value(&empty).unwrap();
        assert!(json.get("nowcast").is_none());
        assert!(json.get("forecast").is_none());
        assert_eq!(json["errors"][0], "no coverage");

        // And an AppView with only a nowcast still produces a headline, naming the
        // location exactly once.
        let headline = AppView::build(
            &Location::new("Leiden", 52.16, 4.49),
            now,
            AppParts { nowcast: Some(&nowcast), ..Default::default() },
            12,
            false,
        )
        .headline();
        assert!(headline.starts_with("Leiden: "), "{headline}");
        assert_eq!(headline.matches("Leiden").count(), 1, "{headline}");

        // So does one with only a forecast.
        let headline = AppView::build(
            &Location::new("Leiden", 52.16, 4.49),
            now,
            AppParts { forecast: Some(&forecast()), ..Default::default() },
            12,
            false,
        )
        .headline();
        assert_eq!(headline, "Leiden: 14.2° · Light rain");
    }
}
