//! The rain nowcast: the high-resolution (+0…+120 min) precipitation series the
//! Omarchy bar pill and its popout render.
//!
//! Ported from WeatherMaster's `RainNowcast` / `RainNowcastPoint`, including the
//! buienradar-style intensity colour ramp (exposed here as [`IntensityClass`] so
//! the QML side can map classes onto Omarchy theme colours).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// KNMI/DWD nowcast window: 25 steps of 5 minutes.
pub const NOWCAST_WINDOW_MINUTES: i64 = 120;

/// Radar context shown before "now", in minutes.
pub const NOWCAST_PAST_CONTEXT_MINUTES: i64 = 15;

/// From this intensity up (mm/h) precipitation counts as rain.
pub const RAIN_THRESHOLD_MM_H: f64 = 0.1;

/// Step length of KNMI/DWD radar nowcasts.
pub const RADAR_SLOT_MINUTES: u32 = 5;

/// Step length of the Open-Meteo `minutely_15` fallback.
pub const MODEL_SLOT_MINUTES: u32 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NowcastProvider {
    /// KNMI `radar_forecast_2.0` via WMS GetFeatureInfo — the Netherlands.
    Knmi,
    /// DWD `Radarkomposit RV` via Bright Sky — Germany.
    Dwd,
    /// Open-Meteo `minutely_15` model precipitation — worldwide, keyless.
    OpenMeteo,
}

impl NowcastProvider {
    pub fn display_name(self) -> &'static str {
        match self {
            NowcastProvider::Knmi => "KNMI",
            NowcastProvider::Dwd => "DWD",
            NowcastProvider::OpenMeteo => "Open-Meteo",
        }
    }

    /// CC BY 4.0 attribution line shown next to the chart.
    pub fn attribution(self) -> &'static str {
        match self {
            NowcastProvider::Knmi => "Radar © KNMI",
            NowcastProvider::Dwd => "Radar © DWD (Bright Sky)",
            NowcastProvider::OpenMeteo => "Model © Open-Meteo",
        }
    }

    pub fn is_radar(self) -> bool {
        matches!(self, NowcastProvider::Knmi | NowcastProvider::Dwd)
    }
}

/// Intensity buckets behind the bar chart's colour ramp (mm/h).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntensityClass {
    Dry,
    Drizzle,
    Light,
    Moderate,
    Heavy,
    VeryHeavy,
    Extreme,
}

impl IntensityClass {
    pub fn from_mm_per_hour(mm_per_hour: f64) -> Self {
        match mm_per_hour {
            v if v <= 0.0 => IntensityClass::Dry,
            v if v < 0.3 => IntensityClass::Drizzle,
            v if v < 1.0 => IntensityClass::Light,
            v if v < 3.0 => IntensityClass::Moderate,
            v if v < 10.0 => IntensityClass::Heavy,
            v if v < 25.0 => IntensityClass::VeryHeavy,
            _ => IntensityClass::Extreme,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            IntensityClass::Dry => "dry",
            IntensityClass::Drizzle => "drizzle",
            IntensityClass::Light => "light",
            IntensityClass::Moderate => "moderate",
            IntensityClass::Heavy => "heavy",
            IntensityClass::VeryHeavy => "very heavy",
            IntensityClass::Extreme => "extreme",
        }
    }
}

/// Shorthand used by callers that only have an intensity.
pub fn intensity_class(mm_per_hour: f64) -> IntensityClass {
    IntensityClass::from_mm_per_hour(mm_per_hour)
}

/// One high-resolution precipitation sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NowcastPoint {
    /// UTC start of the interval.
    pub time: DateTime<Utc>,
    /// Rain rate in mm/h. Always mm/h internally, whatever the provider emits.
    pub intensity_mm_per_hour: f64,
    /// Length of the interval this sample covers (5 for radar, 15 for the model).
    pub interval_minutes: u32,
}

impl NowcastPoint {
    /// Water accumulated over this interval, in mm.
    pub fn millimeters(&self) -> f64 {
        self.intensity_mm_per_hour * f64::from(self.interval_minutes) / 60.0
    }

    pub fn class(&self) -> IntensityClass {
        IntensityClass::from_mm_per_hour(self.intensity_mm_per_hour)
    }
}

/// A high-resolution rain nowcast for roughly the next two hours.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RainNowcast {
    pub points: Vec<NowcastPoint>,
    pub provider: NowcastProvider,
    pub is_radar_based: bool,
    pub fetched_at: DateTime<Utc>,
}

impl RainNowcast {
    /// Sorts the samples and stamps the fetch time.
    pub fn new(
        mut points: Vec<NowcastPoint>,
        provider: NowcastProvider,
        is_radar_based: bool,
    ) -> Self {
        points.sort_by_key(|p| p.time);
        Self { points, provider, is_radar_based, fetched_at: Utc::now() }
    }

    /// Total accumulation over the covered window, in mm.
    pub fn total_millimeters(&self) -> f64 {
        self.points.iter().map(NowcastPoint::millimeters).sum()
    }

    pub fn max_intensity(&self) -> f64 {
        self.points.iter().map(|p| p.intensity_mm_per_hour).fold(0.0, f64::max)
    }

    pub fn slot_minutes(&self) -> u32 {
        self.points.first().map_or(RADAR_SLOT_MINUTES, |p| p.interval_minutes)
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The most recent sample at or before `now` (the one "now" sits in).
    pub fn point_at(&self, now: DateTime<Utc>) -> Option<&NowcastPoint> {
        self.points.iter().rev().find(|p| p.time <= now).or_else(|| self.points.first())
    }

    /// Everything the bar pill and the popout header need, in one call.
    pub fn summary(&self, now: DateTime<Utc>) -> NowcastSummary {
        let current = self.point_at(now).map_or(0.0, |p| p.intensity_mm_per_hour);
        let is_raining_now = current >= RAIN_THRESHOLD_MM_H;

        // First sample at or after `now` that counts as rain.
        let first_rainy = if is_raining_now {
            None
        } else {
            self.points
                .iter()
                .find(|p| p.time >= now && p.intensity_mm_per_hour >= RAIN_THRESHOLD_MM_H)
        };
        let starts_in_minutes = if is_raining_now {
            None
        } else {
            first_rainy.map(|p| (p.time - now).num_minutes().max(0))
        };

        // Walk from "now" (or from the first rainy sample) to the first dry gap.
        let walk_from = if is_raining_now { now } else { first_rainy.map_or(now, |p| p.time) };
        let ends_in_minutes = if is_raining_now || first_rainy.is_some() {
            self.points
                .iter()
                .filter(|p| p.time >= walk_from)
                .find(|p| p.intensity_mm_per_hour < RAIN_THRESHOLD_MM_H)
                .map(|p| (p.time - now).num_minutes().max(0))
        } else {
            None
        };

        let peak = self
            .points
            .iter()
            .max_by(|a, b| a.intensity_mm_per_hour.total_cmp(&b.intensity_mm_per_hour));

        NowcastSummary {
            is_raining_now,
            current_mm_per_hour: current,
            current_class: IntensityClass::from_mm_per_hour(current),
            starts_in_minutes,
            ends_in_minutes,
            total_millimeters: self.total_millimeters(),
            peak_mm_per_hour: peak.map_or(0.0, |p| p.intensity_mm_per_hour),
            peak_in_minutes: peak.map(|p| (p.time - now).num_minutes()),
            slot_minutes: self.slot_minutes(),
            provider: self.provider,
            is_radar_based: self.is_radar_based,
        }
    }
}

/// Pre-computed state for the bar pill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NowcastSummary {
    pub is_raining_now: bool,
    pub current_mm_per_hour: f64,
    pub current_class: IntensityClass,
    /// Minutes until rain starts, when it is not raining yet.
    pub starts_in_minutes: Option<i64>,
    /// Minutes until the current or upcoming rain ends, when it stops inside the window.
    pub ends_in_minutes: Option<i64>,
    pub total_millimeters: f64,
    pub peak_mm_per_hour: f64,
    /// Minutes until the peak, negative when the peak is already past.
    pub peak_in_minutes: Option<i64>,
    pub slot_minutes: u32,
    pub provider: NowcastProvider,
    pub is_radar_based: bool,
}

impl NowcastSummary {
    /// Short pill text, e.g. `0.40 mm/h`, `in 25m`, `dry`. The rate carries its unit and its
    /// conversion together, so an imperial reader is not shown millimetres in a hat.
    pub fn pill_text(&self, imperial: bool, style: DurationStyle) -> String {
        if self.is_raining_now {
            crate::units::format_rate(self.current_mm_per_hour, imperial)
        } else if let Some(minutes) = self.starts_in_minutes {
            format!("in {}", format_minutes(minutes, style))
        } else {
            "dry".to_string()
        }
    }
}

/// Upstream's decimals rule: two decimals below 1 mm/h, one above.
pub fn format_intensity(mm_per_hour: f64) -> String {
    if mm_per_hour < 1.0 {
        format!("{mm_per_hour:.2}")
    } else {
        format!("{mm_per_hour:.1}")
    }
}

/// How a span of minutes is written: the one setting behind every "in 25m" and "1:31h" the
/// pill, the popout, the window and the CLI show. One style, one format, everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DurationStyle {
    /// `25m`, `1:31h` — the continental way of writing a time span.
    #[default]
    Colon,
    /// `25 min`, `1 h 31 min`.
    Words,
    /// `25m`, `1h31`.
    Compact,
}

impl DurationStyle {
    pub const ALL: [DurationStyle; 3] = [Self::Colon, Self::Words, Self::Compact];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Colon => "colon",
            Self::Words => "words",
            Self::Compact => "compact",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.as_str().eq_ignore_ascii_case(text.trim()))
    }
}

/// `25m` / `1:05h` (colon), `25 min` / `1 h 05 min` (words), `25m` / `1h05` (compact).
pub fn format_minutes(minutes: i64, style: DurationStyle) -> String {
    let minutes = minutes.max(0);
    let (hours, rest) = (minutes / 60, minutes % 60);
    match style {
        DurationStyle::Colon if hours == 0 => format!("{rest}m"),
        DurationStyle::Colon => format!("{hours}:{rest:02}h"),
        DurationStyle::Words if hours == 0 => format!("{rest} min"),
        DurationStyle::Words => format!("{hours} h {rest:02} min"),
        DurationStyle::Compact if hours == 0 => format!("{rest}m"),
        DurationStyle::Compact => format!("{hours}h{rest:02}"),
    }
}

/// Total accumulation in mm rendered in the user's precipitation unit.
///
/// Kept as a name because it is what the front-ends already call; the work is in `units`.
pub fn format_millimeters(mm: f64, imperial: bool) -> String {
    crate::units::format_millimetres(mm, imperial)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn at(minute: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 11, 15, 0, 0).unwrap() + Duration::minutes(minute)
    }

    fn rain(points: &[(i64, f64)]) -> RainNowcast {
        RainNowcast::new(
            points
                .iter()
                .map(|(minute, intensity)| NowcastPoint {
                    time: at(*minute),
                    intensity_mm_per_hour: *intensity,
                    interval_minutes: 5,
                })
                .collect(),
            NowcastProvider::Knmi,
            true,
        )
    }

    #[test]
    fn total_and_peak_match_manual_maths() {
        let nowcast = rain(&[(0, 0.0), (5, 1.2), (10, 2.4), (15, 0.0)]);
        // 1.2 and 2.4 mm/h over 5 minutes = 0.1 + 0.2 mm.
        assert!((nowcast.total_millimeters() - 0.3).abs() < 1e-9);
        assert!((nowcast.max_intensity() - 2.4).abs() < 1e-9);
    }

    #[test]
    fn summary_reports_rain_already_falling() {
        let now = at(6);
        let summary = rain(&[(0, 0.0), (5, 1.0), (10, 1.0), (15, 0.0)]).summary(now);
        assert!(summary.is_raining_now);
        assert_eq!(summary.starts_in_minutes, None);
        assert_eq!(summary.ends_in_minutes, Some(9)); // rain stops at +15, now is +6
    }

    #[test]
    fn summary_reports_rain_starting_later() {
        let now = at(2);
        let summary = rain(&[(0, 0.0), (5, 0.0), (10, 0.8), (15, 0.0)]).summary(now);
        assert!(!summary.is_raining_now);
        assert_eq!(summary.starts_in_minutes, Some(8)); // rain starts at +10
        assert_eq!(summary.ends_in_minutes, Some(13));
        assert_eq!(summary.pill_text(false, DurationStyle::Words), "in 8 min");
    }

    #[test]
    fn summary_stays_dry_when_nothing_crosses_the_threshold() {
        let now = at(2);
        let summary = rain(&[(0, 0.05), (5, 0.05), (10, 0.0)]).summary(now);
        assert!(!summary.is_raining_now);
        assert_eq!(summary.starts_in_minutes, None);
        assert_eq!(summary.ends_in_minutes, None);
        assert_eq!(summary.pill_text(false, DurationStyle::Colon), "dry");
    }

    #[test]
    fn intensity_classes_follow_the_buienradar_ramp() {
        assert_eq!(IntensityClass::from_mm_per_hour(0.0), IntensityClass::Dry);
        assert_eq!(IntensityClass::from_mm_per_hour(0.2), IntensityClass::Drizzle);
        assert_eq!(IntensityClass::from_mm_per_hour(0.9), IntensityClass::Light);
        assert_eq!(IntensityClass::from_mm_per_hour(2.9), IntensityClass::Moderate);
        assert_eq!(IntensityClass::from_mm_per_hour(9.9), IntensityClass::Heavy);
        assert_eq!(IntensityClass::from_mm_per_hour(24.9), IntensityClass::VeryHeavy);
        assert_eq!(IntensityClass::from_mm_per_hour(25.0), IntensityClass::Extreme);
    }

    #[test]
    fn formatting_matches_upstream() {
        assert_eq!(format_intensity(0.4), "0.40");
        assert_eq!(format_intensity(2.44), "2.4");
        use DurationStyle::*;
        assert_eq!(format_minutes(25, Colon), "25m");
        assert_eq!(format_minutes(91, Colon), "1:31h");
        assert_eq!(format_minutes(120, Colon), "2:00h");
        assert_eq!(format_minutes(45, Words), "45 min");
        assert_eq!(format_minutes(91, Words), "1 h 31 min");
        assert_eq!(format_minutes(65, Compact), "1h05");
        assert_eq!(format_minutes(25, Compact), "25m");
        assert_eq!(DurationStyle::parse(" Words "), Some(Words));
        assert_eq!(DurationStyle::parse("nope"), None);
        assert_eq!(format_millimeters(1.0, false), "1.00 mm");
        assert_eq!(format_millimeters(25.4, true), "1.000 in");
    }
}
