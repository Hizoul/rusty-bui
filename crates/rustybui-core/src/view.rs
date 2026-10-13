//! The wire contract between the Rust core and the QML front-ends (the Omarchy
//! bar pill, its popout, and the standalone app).
//!
//! Everything a view needs is pre-computed here — labels, classes, pill text —
//! so QML never has to duplicate domain rules.

use crate::location::Location;
use crate::nowcast::{
    format_intensity, format_minutes, IntensityClass, NowcastProvider, RainNowcast,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationView {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub country_code: Option<String>,
    pub timezone: Option<String>,
}

impl From<&Location> for LocationView {
    fn from(location: &Location) -> Self {
        Self {
            name: location.name.clone(),
            latitude: location.latitude,
            longitude: location.longitude,
            country_code: location.country_code.clone(),
            timezone: location.timezone.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    /// Stable id for QML conditions: `knmi`, `dwd`, `open-meteo`.
    pub id: String,
    pub label: String,
    pub attribution: String,
    pub is_radar: bool,
}

impl From<NowcastProvider> for ProviderView {
    fn from(provider: NowcastProvider) -> Self {
        let id = match provider {
            NowcastProvider::Knmi => "knmi",
            NowcastProvider::Dwd => "dwd",
            NowcastProvider::OpenMeteo => "open-meteo",
        };
        Self {
            id: id.to_string(),
            label: provider.display_name().to_string(),
            attribution: provider.attribution().to_string(),
            is_radar: provider.is_radar(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryView {
    pub is_raining_now: bool,
    /// Short pill text: `0.40 mm/h`, `in 25 min` or `dry`.
    pub pill: String,
    pub current_mm_per_hour: f64,
    pub current_class: IntensityClass,
    pub current_label: String,
    pub starts_in_minutes: Option<i64>,
    pub starts_in_label: Option<String>,
    pub ends_in_minutes: Option<i64>,
    pub ends_in_label: Option<String>,
    pub total_mm: f64,
    pub peak_mm_per_hour: f64,
    pub peak_in_minutes: Option<i64>,
    pub peak_in_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointView {
    pub time: DateTime<Utc>,
    /// The same instant as `time`, as epoch milliseconds. QML's JS engine cannot
    /// parse nanosecond RFC3339 reliably, so the front-ends use this.
    pub time_ms: i64,
    pub mm_per_hour: f64,
    pub interval_minutes: u32,
    pub class: IntensityClass,
    /// Minutes relative to `now`; negative for the past-context samples.
    pub offset_minutes: i64,
    /// True on the sample that "now" falls into, so the chart can mark it.
    pub is_now: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowcastView {
    pub location: LocationView,
    pub provider: ProviderView,
    pub slot_minutes: u32,
    pub fetched_at: DateTime<Utc>,
    pub fetched_at_ms: i64,
    pub now: DateTime<Utc>,
    pub now_ms: i64,
    pub summary: SummaryView,
    pub points: Vec<PointView>,
}

impl NowcastView {
    pub fn new(
        location: &Location,
        nowcast: &RainNowcast,
        now: DateTime<Utc>,
        imperial: bool,
    ) -> Self {
        let summary = nowcast.summary(now);
        let current_time = nowcast.point_at(now).map(|point| point.time);

        let points = nowcast
            .points
            .iter()
            .map(|point| PointView {
                time: point.time,
                time_ms: point.time.timestamp_millis(),
                mm_per_hour: point.intensity_mm_per_hour,
                interval_minutes: point.interval_minutes,
                class: point.class(),
                offset_minutes: (point.time - now).num_minutes(),
                is_now: current_time == Some(point.time),
            })
            .collect();

        Self {
            location: LocationView::from(location),
            provider: ProviderView::from(nowcast.provider),
            slot_minutes: nowcast.slot_minutes(),
            fetched_at: nowcast.fetched_at,
            fetched_at_ms: nowcast.fetched_at.timestamp_millis(),
            now,
            now_ms: now.timestamp_millis(),
            summary: SummaryView {
                is_raining_now: summary.is_raining_now,
                pill: summary.pill_text(imperial),
                current_mm_per_hour: summary.current_mm_per_hour,
                current_class: summary.current_class,
                current_label: summary.current_class.label().to_string(),
                starts_in_minutes: summary.starts_in_minutes,
                starts_in_label: summary.starts_in_minutes.map(format_minutes),
                ends_in_minutes: summary.ends_in_minutes,
                ends_in_label: summary.ends_in_minutes.map(format_minutes),
                total_mm: summary.total_millimeters,
                peak_mm_per_hour: summary.peak_mm_per_hour,
                peak_in_minutes: summary.peak_in_minutes,
                peak_in_label: summary.peak_in_minutes.map(format_minutes),
            },
            points,
        }
    }

    /// One-line description for `omarchy-weather`-style notifications.
    pub fn sentence(&self) -> String {
        let mut parts = vec![format!("{}: {}", self.location.name, self.summary.pill)];
        if let Some(ends) = &self.summary.ends_in_label {
            parts.push(format!("rain stops in {ends}"));
        }
        if self.summary.total_mm > 0.0 {
            parts.push(format!("{:.2} mm expected", self.summary.total_mm));
        }
        parts.push(format!("{} · {}", self.provider.label, self.provider.attribution));
        parts.join(" · ")
    }

    /// Waybar/Omarchy inline-module JSON: plain text plus a tooltip.
    pub fn bar_text(&self) -> String {
        if self.summary.is_raining_now {
            format!("{} mm/h", format_intensity(self.summary.current_mm_per_hour))
        } else if let Some(starts) = &self.summary.starts_in_label {
            format!("in {starts}")
        } else {
            "dry".to_string()
        }
    }

    pub fn bar_class(&self) -> String {
        if self.summary.is_raining_now {
            format!("rain-{}", self.summary.current_class.label().replace(' ', "-"))
        } else if self.summary.starts_in_minutes.is_some() {
            "rain-soon".to_string()
        } else {
            "dry".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nowcast::{NowcastPoint, NowcastProvider};
    use chrono::TimeZone;

    fn view() -> NowcastView {
        let now = Utc.with_ymd_and_hms(2026, 10, 11, 15, 2, 0).unwrap();
        let nowcast = RainNowcast::new(
            vec![
                NowcastPoint {
                    time: now,
                    intensity_mm_per_hour: 0.0,
                    interval_minutes: 5,
                },
                NowcastPoint {
                    time: now + chrono::Duration::minutes(10),
                    intensity_mm_per_hour: 1.4,
                    interval_minutes: 5,
                },
            ],
            NowcastProvider::Knmi,
            true,
        );
        NowcastView::new(&Location::new("Leiden", 52.16, 4.49), &nowcast, now, false)
    }

    #[test]
    fn view_serialises_camel_case_and_preserialises_labels() {
        let json = serde_json::to_value(view()).unwrap();
        assert_eq!(json["provider"]["id"], "knmi");
        assert_eq!(json["provider"]["attribution"], "Radar © KNMI");
        assert_eq!(json["summary"]["pill"], "in 10 min");
        assert_eq!(json["slotMinutes"], 5);
        assert_eq!(json["points"][0]["isNow"], true);
        assert_eq!(json["points"][1]["class"], "moderate");
        // QML never parses timestamps, so every instant also ships as millis.
        assert!(json["nowMs"].is_i64());
        assert!(json["fetchedAtMs"].is_i64());
        assert_eq!(json["points"][0]["timeMs"], json["nowMs"]);
        assert_eq!(json["points"][1]["timeMs"].as_i64().unwrap() - json["nowMs"].as_i64().unwrap(), 600_000);
    }

    #[test]
    fn bar_text_matches_the_pill() {
        let view = view();
        assert_eq!(view.bar_text(), "in 10 min");
        assert_eq!(view.bar_class(), "rain-soon");
        assert!(view.sentence().contains("Leiden: in 10 min"));
        assert!(view.sentence().contains("Radar © KNMI"));
    }
}
