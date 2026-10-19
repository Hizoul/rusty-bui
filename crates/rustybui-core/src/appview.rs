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
use crate::nowcast::{DurationStyle, RainNowcast};
use crate::providers::openmeteo::AirQuality;
use crate::providers::ForecastProvider;
use crate::view::{LocationView, NowcastView};
use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};
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

/// `14.2°`, or `57.6°F` — the label and its unit are built together in `units`, so a preference
/// cannot be applied to one and forgotten on the other.
fn temperature(value: Option<f64>, imperial: bool) -> Option<String> {
    value.map(|celsius| crate::units::format_temperature(celsius, imperial))
}

fn millimetres(value: f64, imperial: bool) -> String {
    crate::units::format_millimetres(value, imperial)
}

fn speed(value: f64, imperial: bool) -> String {
    crate::units::format_speed(value, imperial)
}

fn clock(time: DateTime<FixedOffset>, clock24: bool) -> String {
    if clock24 {
        time.format("%H:%M").to_string()
    } else {
        time.format("%-I:%M %p").to_string()
    }
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
    /// The apparent temperature for the hour, e.g. `11.2°` — the number only, the strip says it.
    pub feels_like_label: Option<String>,
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
    pub fn new(
     forecast: &Forecast,
     location: &Location,
     now: DateTime<Utc>,
     hours: usize,
     imperial: bool,
     clock24: bool,
 ) -> Self {
        let offset = FixedOffset::east_opt(forecast.utc_offset_seconds as i32)
            .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());

        let hourly = forecast
            .hourly
            .iter()
            .filter(|sample| sample.time + chrono::Duration::hours(1) > now)
            .take(hours)
            .map(|sample| hourly_view(sample, location, offset, imperial, clock24))
            .collect();

        let daily = forecast
            .daily
            .iter()
            .map(|day_forecast| daily_view(day_forecast, location, offset, imperial, clock24))
            .collect();

        Self {
            timezone: forecast.timezone.clone(),
            utc_offset_seconds: forecast.utc_offset_seconds,
            current: forecast
                .current
                .as_ref()
                .map(|current| current_view(current, offset, imperial, clock24)),
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
    clock24: bool,
) -> CurrentView {
    CurrentView {
        time_ms: current.time.map(|time| time.timestamp_millis()),
        clock_label: current
            .time
            .map(|time| clock(time.with_timezone(&offset), clock24)),
        temperature_c: current.temperature_c,
        temperature_label: temperature(current.temperature_c, imperial),
        feels_like_label: current
            .apparent_temperature_c
            .map(|value| format!("feels {}", crate::units::format_temperature(value, imperial))),
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
            .map(|value| crate::units::format_pressure(value, imperial)),
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

fn hourly_view(
    sample: &HourlyForecast,
    location: &Location,
    offset: FixedOffset,
    imperial: bool,
    clock24: bool,
) -> HourlyView {
    let local = sample.time.with_timezone(&offset);
    HourlyView {
        time_ms: sample.time.timestamp_millis(),
        clock_label: clock(local, clock24),
        temperature_c: sample.temperature_c,
        temperature_label: temperature(sample.temperature_c, imperial),
        feels_like_label: sample.apparent_temperature_c.map(|value| {
            crate::units::format_temperature(value, imperial)
        }),
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
        is_daylight: daylight_at(sample.time, location),
    }
}

/// Whether the sun is up at `at`, from the arithmetic rather than from a provider's flag: a
/// glyph that only knew 07:00–19:00 would draw a sunset in Berlin in June at nine in the evening
/// as if it were night.
fn daylight_at(at: DateTime<Utc>, location: &Location) -> bool {
    let date = at.date_naive();
    match crate::solar::times_on(date, location.latitude, location.longitude) {
        Some(times) => at >= times.sunrise && at < times.sunset,
        // A polar day has no sunset to be before and a polar night no sunrise to be after;
        // the clock is the only thing left to go on, and the two cases differ by season.
        None => {
            // No crossing of the horizon at all: either the sun never sets or it never rises.
            // Which one follows from the hemisphere and the season, and inside a polar day the
            // clock is all there is left to go on. Nobody this app maps lives up here; the
            // honesty is in not inventing a sunrise that does not exist.
            let polar_day = (location.latitude > 0.0) == (3..=9).contains(&date.month());
            polar_day && (7..=19).contains(&at.hour())
        }
    }
}

fn daily_view(
    day_forecast: &DailyForecast,
    location: &Location,
    offset: FixedOffset,
    imperial: bool,
    clock24: bool,
) -> DailyView {
    let local = day_forecast.time.with_timezone(&offset);
    // The day's own sun times, computed when the provider's are missing: Bright Sky publishes
    // none, and a daily row with a blank sunrise is worse than one that worked it out.
    let computed = chrono::NaiveDate::parse_from_str(&day_forecast.date, "%Y-%m-%d")
        .ok()
        .and_then(|date| crate::solar::times_on(date, location.latitude, location.longitude));

    DailyView {
        time_ms: day_forecast.time.timestamp_millis(),
        date: day_forecast.date.clone(),
        day_label: day(local),
        high_label: temperature(day_forecast.temperature_max_c, imperial),
        low_label: temperature(day_forecast.temperature_min_c, imperial),
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
            .or_else(|| computed.map(|times| times.sunrise))
            .map(|time| clock(time.with_timezone(&offset), clock24)),
        sunset_label: day_forecast
            .sunset
            .or_else(|| computed.map(|times| times.sunset))
            .map(|time| clock(time.with_timezone(&offset), clock24)),
    }
}

/// One day of forecast, opened up: what a daily row in the window's list expands into.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayView {
    pub location: LocationView,
    /// `2026-10-15` — the day that was asked for.
    pub date: String,
    /// `Thu 15 Oct`, in the location's own calendar, taken from the day row when there is one.
    pub day_label: String,
    pub timezone: String,
    /// That day's row in the daily list: its high and low, its rain, its sun.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<DailyView>,
    /// One row per hour of that day, in the location's own clock.
    pub hours: Vec<HourlyView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast_attribution: Option<String>,
}

impl DayView {
    pub fn build(
        location: &Location,
        forecast: &Forecast,
        date: chrono::NaiveDate,
        provider: ForecastProvider,
        imperial: bool,
        clock24: bool,
    ) -> Self {
        let offset = FixedOffset::east_opt(forecast.utc_offset_seconds as i32)
            .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
        let wanted = date.format("%Y-%m-%d").to_string();

        let day = forecast
            .daily
            .iter()
            .find(|day| day.date == wanted)
            .or_else(|| forecast.daily.first())
            .map(|day| daily_view(day, location, offset, imperial, clock24));

        // The hours are filtered to the day as well as asked for it: a provider that answers
        // with a range either side of the date would otherwise put two midnights in one list.
        let hours: Vec<HourlyView> = forecast
            .hourly
            .iter()
            .filter(|sample| sample.time.with_timezone(&offset).date_naive() == date)
            .map(|sample| hourly_view(sample, location, offset, imperial, clock24))
            .collect();

        Self {
            location: LocationView::from(location),
            date: wanted,
            day_label: day
                .as_ref()
                .map(|day| day.day_label.clone())
                .unwrap_or_else(|| date.format("%a %-d %b").to_string()),
            timezone: forecast.timezone.clone(),
            day,
            hours,
            forecast_attribution: Some(provider.attribution().to_string()),
        }
    }
}

/// The sun and the moon for today: when the light goes, and what the moon is doing.
///
/// The moon is computed, not fetched — none of the three providers publishes it — and it is
/// phase and illumination only. Moonrise and moonset would need an ephemeris per place, and
/// half a feature beats an invented one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkyView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sunrise_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sunset_label: Option<String>,
    /// `10h 57m`, or nothing when the provider gave no times.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daylight_label: Option<String>,
    /// How far through today's daylight it is: 0 at sunrise, 1 at sunset. Absent at night, so
    /// a bar can be shown only while it means something.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daylight_progress: Option<f64>,
    /// `sets in 3h 20m`, or `rises in 9h 10m` before dawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sun_event_label: Option<String>,
    pub is_daylight: bool,
    /// `new`, `waxing-crescent`, … for a front-end to draw its own disc and pick its own glyph.
    pub moon_phase: String,
    pub moon_phase_label: String,
    /// 0–1 of the disc that the sun is lighting.
    pub moon_illumination: f64,
    pub moon_label: String,
    pub moon_waxing: bool,
}

impl SkyView {
    fn new(
        day: Option<&DailyForecast>,
        location: &Location,
        now: DateTime<Utc>,
        offset: FixedOffset,
        clock24: bool,
    ) -> Self {
        // The provider's own times where it has them; the arithmetic where it does not, which is
        // every DWD day.
        let computed = day
            .and_then(|day| chrono::NaiveDate::parse_from_str(&day.date, "%Y-%m-%d").ok())
            .and_then(|date| crate::solar::times_on(date, location.latitude, location.longitude));
        let sunrise = day.and_then(|day| day.sunrise).or_else(|| computed.map(|times| times.sunrise));
        let sunset = day.and_then(|day| day.sunset).or_else(|| computed.map(|times| times.sunset));
        let moon = crate::moon::phase_at(now);

        let daylight = match (sunrise, sunset) {
            (Some(rise), Some(set)) if set > rise => Some(set - rise),
            _ => day
                .and_then(|day| day.daylight_seconds)
                .map(|seconds| chrono::Duration::seconds(seconds as i64)),
        };

        // The sun is up when it is between the two, which is the only thing the times can tell
        // us — a provider's own `is_day` is for the hour it was observed, not for now.
        let is_daylight = matches!((sunrise, sunset), (Some(rise), Some(set)) if now >= rise && now < set);
        let daylight_progress = match (sunrise, sunset) {
            (Some(rise), Some(set)) if set > rise && now >= rise && now < set => {
                Some((now - rise).num_seconds() as f64 / (set - rise).num_seconds() as f64)
            }
            _ => None,
        };
        let sun_event_label = match (sunrise, sunset) {
            (Some(rise), _) if now < rise => Some(format!("rises in {}", duration_words(rise - now))),
            (_, Some(set)) if now < set => Some(format!("sets in {}", duration_words(set - now))),
            _ => None,
        };

        Self {
            sunrise_label: sunrise.map(|time| clock(time.with_timezone(&offset), clock24)),
            sunset_label: sunset.map(|time| clock(time.with_timezone(&offset), clock24)),
            daylight_label: daylight.map(duration_words),
            daylight_progress,
            sun_event_label,
            is_daylight,
            moon_phase: moon.phase_name.key().to_string(),
            moon_phase_label: moon.phase_name.label().to_string(),
            moon_illumination: moon.illumination,
            moon_label: moon.illumination_label(),
            moon_waxing: moon.waxing,
        }
    }
}

/// `3h 20m`, `12m` — a wait, in the words a person would use for it.
fn duration_words(span: chrono::Duration) -> String {
    let minutes = span.num_minutes().max(0);
    let (hours, minutes) = (minutes / 60, minutes % 60);
    if hours == 0 {
        format!("{minutes}m")
    } else {
        format!("{hours}h {minutes:02}m")
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
    pub fn new(alert: &Alert, offset: FixedOffset, clock24: bool) -> Self {
        let at = |value: Option<DateTime<Utc>>| {
            value.map(|time| clock(time.with_timezone(&offset), clock24))
        };
        let window_label = match (at(alert.onset), at(alert.expires)) {
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
    /// The sun and the moon, which come from the forecast's first day and from arithmetic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky: Option<SkyView>,
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
        clock24: bool,
        style: DurationStyle,
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
                .map(|alert| AlertView::new(alert, offset, clock24))
                .collect(),
            nowcast: parts
                .nowcast
                .map(|nowcast| NowcastView::new(location, nowcast, now, imperial, style)),
            forecast: parts
                .forecast
                .map(|forecast| ForecastView::new(forecast, location, now, hours, imperial, clock24)),
            forecast_attribution: parts
                .forecast_provider
                .map(|provider| provider.attribution().to_string()),
            air: parts.air.map(AirQualityView::from),
            sky: parts.forecast.map(|forecast| {
                SkyView::new(forecast.daily.first(), location, now, offset, clock24)
            }),
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
            &Location::new("Leiden", 52.16, 4.49),
            Utc.with_ymd_and_hms(2025, 10, 11, 15, 0, 0).unwrap(),
            12,
            false,
            true,
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
            &Location::new("Leiden", 52.16, 4.49),
            Utc.with_ymd_and_hms(2025, 10, 11, 15, 0, 0).unwrap(),
            12,
            true,
            true,
        );
        let current = imperial.current.as_ref().unwrap();
        assert_eq!(current.wind_label.as_deref(), Some("11.4 mph SSW"));
        assert_eq!(current.temperature_label.as_deref(), Some("57.6°F"));
        assert_eq!(current.feels_like_label.as_deref(), Some("feels 55.6°F"));
        assert_eq!(current.pressure_label.as_deref(), Some("29.90 inHg"));
        assert_eq!(current.precipitation_label.as_deref(), Some("0.012 in"));

        // The hourly strip and the day rows carry temperatures too, and they are built by
        // different functions — the ones that quietly stayed metric while the current block
        // converted.
        let hourly = ForecastView::new(
            &forecast(),
            &Location::new("Leiden", 52.16, 4.49),
            Utc.with_ymd_and_hms(2025, 10, 11, 15, 0, 0).unwrap(),
            12,
            true,
            true,
        );
        assert_eq!(hourly.hourly[0].temperature_label.as_deref(), Some("58.3°F"));
        assert_eq!(hourly.daily[0].high_label.as_deref(), Some("59.2°F"));
        assert_eq!(hourly.daily[0].low_label.as_deref(), Some("48.6°F"));
    }

    #[test]
    fn hourly_and_daily_rows_are_clocked_in_the_locations_own_offset() {
        let now = Utc.with_ymd_and_hms(2025, 10, 11, 15, 0, 0).unwrap();
        let view = ForecastView::new(&forecast(), &Location::new("Leiden", 52.16, 4.49), now, 12, false, true);

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
    fn a_day_view_opens_one_day_of_the_forecast() {
        let location = Location::new("Leiden", 52.16, 4.49);
        let forecast = forecast();
        let date = chrono::NaiveDate::from_ymd_opt(2025, 10, 11).unwrap();

        let view = DayView::build(&location, &forecast, date, ForecastProvider::OpenMeteo, false, true);

        assert_eq!(view.date, "2025-10-11");
        assert_eq!(view.day_label, "Sat 11 Oct");
        assert_eq!(view.timezone, "Europe/Amsterdam");
        // The fixture's day carries the provider's own sunrise and sunset, which the detail
        // keeps rather than recomputing.
        let day = view.day.as_ref().expect("the day row");
        assert_eq!(day.sunrise_label.as_deref(), Some("07:50"));
        assert_eq!(day.sunset_label.as_deref(), Some("18:51"));
        // And the hours are the ones that fall on that date, in the location's clock.
        assert_eq!(view.hours.len(), 2);
        assert_eq!(view.hours[0].clock_label, "16:00");
        assert_eq!(view.hours[0].temperature_label.as_deref(), Some("14.2°"));
        assert_eq!(view.hours[1].clock_label, "17:00");

        // A day the provider has nothing for still answers: the rows are empty rather than
        // wrong, and the label falls back to the date itself.
        let far = DayView::build(
            &location,
            &forecast,
            chrono::NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
            ForecastProvider::OpenMeteo,
            false,
            true,
        );
        assert!(far.hours.is_empty());
        assert_eq!(far.date, "2026-03-03");
    }

    #[test]
    fn the_sky_block_reads_the_day_and_the_clock_off_the_forecast() {
        let now = Utc.with_ymd_and_hms(2025, 10, 11, 15, 0, 0).unwrap();
        let forecast = forecast();
        let leiden = Location::new("Leiden", 52.16, 4.49);
        let sky = SkyView::new(forecast.daily.first(), &leiden, now, FixedOffset::east_opt(7200).unwrap(), true);

        // 1760161800 = 05:50Z sunrise, 1760201460 = 16:51Z sunset, read in +02:00.
        assert_eq!(sky.sunrise_label.as_deref(), Some("07:50"));
        assert_eq!(sky.sunset_label.as_deref(), Some("18:51"));
        assert_eq!(sky.daylight_label.as_deref(), Some("11h 01m"));
        // 15:00Z is 9h10m into an 11h01m day.
        assert_eq!(sky.daylight_progress.map(|value| (value * 100.0).round()), Some(83.0));
        assert!(sky.is_daylight);
        assert_eq!(sky.sun_event_label.as_deref(), Some("sets in 1h 51m"));

        // And the moon: computed here rather than fetched, and labelled for a disc to be drawn.
        assert!(sky.moon_phase.contains('-') || sky.moon_phase == "new" || sky.moon_phase == "full");
        assert!(!sky.moon_phase_label.is_empty());
        assert!((0.0..=1.0).contains(&sky.moon_illumination));
        assert!(sky.moon_label.ends_with("lit"));

        // At night the bar has nothing to say, and the sun's next event is tomorrow's sunrise —
        // which this block does not know, so it says nothing rather than guessing.
        let night = SkyView::new(
            forecast.daily.first(),
            &leiden,
            Utc.with_ymd_and_hms(2025, 10, 11, 20, 0, 0).unwrap(),
            FixedOffset::east_opt(7200).unwrap(),
            true,
        );
        assert!(!night.is_daylight);
        assert_eq!(night.daylight_progress, None);
        assert_eq!(night.sun_event_label, None);
        assert_eq!(night.daylight_label.as_deref(), Some("11h 01m"));

        // A provider that gives no times at all still yields the moon, and no invention.
        let bare = SkyView::new(None, &leiden, now, FixedOffset::east_opt(0).unwrap(), true);
        assert_eq!(bare.sunrise_label, None);
        assert!(!bare.is_daylight);
        assert!(!bare.moon_phase.is_empty());
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
            true,
            DurationStyle::Colon,
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
            true,
            DurationStyle::Colon,
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
            true,
            DurationStyle::Colon,
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
            true,
            DurationStyle::Colon,
        )
        .headline();
        assert_eq!(headline, "Leiden: 14.2° · Light rain");
    }
}
