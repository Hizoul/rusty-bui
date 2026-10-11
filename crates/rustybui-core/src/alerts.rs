//! Weather warnings.
//!
//! DWD publishes them as CAP messages, and Bright Sky serves the ones that cover a point
//! through `/alerts`, keylessly. KNMI's Dutch warnings, by contrast, live in a keyed
//! dataset on the KNMI Data Platform (its API answers 401 without a key), so the
//! Netherlands has no warning source here yet — a place with no warnings simply has none
//! rather than showing a placeholder.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    /// The CAP message identifier.
    pub id: String,
    /// What is being warned about, e.g. "Wind gusts" — the English text where DWD
    /// publishes one, since that is the language this port's UI speaks.
    pub event: String,
    pub headline: String,
    pub description: Option<String>,
    /// What to do about it, when DWD says.
    pub instruction: Option<String>,
    pub severity: AlertSeverity,
    /// `immediate` or `future`, as published.
    pub urgency: Option<String>,
    pub onset: Option<DateTime<Utc>>,
    pub expires: Option<DateTime<Utc>>,
    /// CC BY 4.0 attribution of the source that issued it.
    pub attribution: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertSeverity {
    Minor,
    Moderate,
    Severe,
    Extreme,
}

impl AlertSeverity {
    /// DWD's four levels. Anything else reads as the mildest rather than as a worse level
    /// than the source claimed.
    pub fn from_published(value: &str) -> Self {
        match value {
            "moderate" => AlertSeverity::Moderate,
            "severe" => AlertSeverity::Severe,
            "extreme" => AlertSeverity::Extreme,
            _ => AlertSeverity::Minor,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AlertSeverity::Minor => "Minor",
            AlertSeverity::Moderate => "Moderate",
            AlertSeverity::Severe => "Severe",
            AlertSeverity::Extreme => "Extreme",
        }
    }

    /// 0-3, for sorting and for the colour band a screen picks.
    pub fn rank(self) -> u8 {
        match self {
            AlertSeverity::Minor => 0,
            AlertSeverity::Moderate => 1,
            AlertSeverity::Severe => 2,
            AlertSeverity::Extreme => 3,
        }
    }
}

/// Maps Bright Sky's `/alerts` answer, worst warning first.
pub fn parse_bright_sky_alerts(body: &str, attribution: &str) -> Result<Vec<Alert>, String> {
    let raw: RawAlerts =
        serde_json::from_str(body).map_err(|error| format!("invalid JSON: {error}"))?;

    let mut alerts: Vec<Alert> = raw
        .alerts
        .into_iter()
        // DWD publishes test messages alongside real ones; a test warning on a real screen
        // is noise rather than information.
        .filter(|raw| raw.status.as_deref() != Some("test"))
        .map(|raw| Alert {
            id: raw.alert_id.unwrap_or_default(),
            event: raw
                .event_en
                .or(raw.event_de)
                .unwrap_or_else(|| "Weather warning".to_string()),
            headline: raw.headline_en.or(raw.headline_de).unwrap_or_default(),
            description: raw.description_en.or(raw.description_de),
            instruction: raw.instruction_en.or(raw.instruction_de),
            severity: AlertSeverity::from_published(raw.severity.as_deref().unwrap_or("")),
            urgency: raw.urgency,
            onset: parse_time(raw.onset.as_deref()),
            expires: parse_time(raw.expires.as_deref()),
            attribution: attribution.to_string(),
        })
        .collect();

    // Worst first, then soonest: whatever a screen shows one of, it shows the one that matters.
    alerts.sort_by(|left, right| {
        right
            .severity
            .rank()
            .cmp(&left.severity.rank())
            .then(left.onset.cmp(&right.onset))
    });

    Ok(alerts)
}

fn parse_time(value: Option<&str>) -> Option<DateTime<Utc>> {
    let value = value?;
    DateTime::parse_from_rfc3339(value).ok().map(|time| time.with_timezone(&Utc))
}

#[derive(Debug, Deserialize)]
struct RawAlerts {
    #[serde(default)]
    alerts: Vec<RawAlert>,
}

#[derive(Debug, Deserialize)]
struct RawAlert {
    alert_id: Option<String>,
    status: Option<String>,
    onset: Option<String>,
    expires: Option<String>,
    severity: Option<String>,
    urgency: Option<String>,
    event_en: Option<String>,
    event_de: Option<String>,
    headline_en: Option<String>,
    headline_de: Option<String>,
    description_en: Option<String>,
    description_de: Option<String>,
    instruction_en: Option<String>,
    instruction_de: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTRIBUTION: &str = "Warnings © DWD (Bright Sky)";

    const SAMPLE: &str = r#"{"alerts":[
      {"id":279977,"alert_id":"2.49.0.0.276.0.DWD.PVW.1691344680000.abc","status":"actual",
       "effective":"2023-08-06T17:58:00+00:00","onset":"2023-08-07T08:00:00+00:00",
       "expires":"2023-08-07T19:00:00+00:00","category":"met","response_type":"prepare",
       "urgency":"future","severity":"severe","certainty":"likely","event_code":311,
       "event_en":"Wind gusts","event_de":"Windböen","headline_en":"Official WARNING of GUSTS",
       "headline_de":"Amtliche WARNUNG vor WINDBÖEN","description_en":"There is a risk of gusts.",
       "description_de":"Es besteht die Gefahr von Windböen.","instruction_en":"Take care.",
       "instruction_de":"Vorsicht."},
      {"alert_id":"2.49.0.0.276.0.DWD.PVW.1691344680001.def","status":"test","severity":"extreme",
       "event_en":"Test warning","onset":"2023-08-07T08:00:00+00:00"},
      {"alert_id":"2.49.0.0.276.0.DWD.PVW.1691344680002.ghi","status":"actual","severity":"minor",
       "event_de":"Nebel","onset":"2023-08-07T06:00:00+00:00","expires":null}
    ],"location":{"warn_cell_id":711000101,"name":"Berl. - Mitte"}}"#;

    #[test]
    fn maps_a_warning_with_english_text_preferred() {
        let alerts = parse_bright_sky_alerts(SAMPLE, ATTRIBUTION).unwrap();
        // The test message is dropped, leaving the severe one and the fog.
        assert_eq!(alerts.len(), 2);

        let worst = &alerts[0];
        assert_eq!(worst.event, "Wind gusts");
        assert_eq!(worst.headline, "Official WARNING of GUSTS");
        assert_eq!(worst.description.as_deref(), Some("There is a risk of gusts."));
        assert_eq!(worst.instruction.as_deref(), Some("Take care."));
        assert_eq!(worst.severity, AlertSeverity::Severe);
        assert_eq!(worst.severity.label(), "Severe");
        assert_eq!(worst.urgency.as_deref(), Some("future"));
        assert_eq!(worst.attribution, ATTRIBUTION);
        assert_eq!(
            worst.onset,
            Some(DateTime::parse_from_rfc3339("2023-08-07T08:00:00+00:00").unwrap().with_timezone(&Utc))
        );
        assert!(worst.expires.is_some());
    }

    #[test]
    fn falls_back_to_german_and_orders_by_severity() {
        let alerts = parse_bright_sky_alerts(SAMPLE, ATTRIBUTION).unwrap();
        let fog = &alerts[1];
        assert_eq!(fog.event, "Nebel", "German text is used when there is no English");
        assert_eq!(fog.severity, AlertSeverity::Minor);
        assert_eq!(fog.expires, None);
        assert!(alerts[0].severity.rank() > alerts[1].severity.rank());
    }

    #[test]
    fn an_unknown_severity_reads_as_the_mildest() {
        assert_eq!(AlertSeverity::from_published("catastrophic"), AlertSeverity::Minor);
        assert_eq!(AlertSeverity::from_published(""), AlertSeverity::Minor);
        assert_eq!(AlertSeverity::from_published("extreme"), AlertSeverity::Extreme);
        assert_eq!(AlertSeverity::from_published("moderate").rank(), 1);
    }

    #[test]
    fn quiet_days_and_garbage_are_reported_separately() {
        let none = parse_bright_sky_alerts(r#"{"alerts":[],"location":null}"#, ATTRIBUTION).unwrap();
        assert!(none.is_empty());
        assert!(parse_bright_sky_alerts("not json", ATTRIBUTION).is_err());
    }
}
