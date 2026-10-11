use serde::{Deserialize, Serialize};

/// KNMI `radar_forecast_2.0` WMS bounding box (the Netherlands). The WMS point
/// query returns no value outside it, so coverage is decided by geography
/// rather than by country code alone.
pub const KNMI_BBOX_MIN_LON: f64 = 3.0;
pub const KNMI_BBOX_MAX_LON: f64 = 7.5;
pub const KNMI_BBOX_MIN_LAT: f64 = 50.5;
pub const KNMI_BBOX_MAX_LAT: f64 = 54.0;

/// A place the app fetches weather for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    /// ISO-3166 alpha-2, when known (used for the KNMI coverage decision).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
    /// IANA zone name; `None` means "use the system zone".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

impl Location {
    pub fn new(name: impl Into<String>, latitude: f64, longitude: f64) -> Self {
        Self { name: name.into(), latitude, longitude, country_code: None, timezone: None }
    }

    pub fn with_country_code(mut self, country_code: impl Into<String>) -> Self {
        self.country_code = Some(country_code.into().to_uppercase());
        self
    }

    pub fn with_timezone(mut self, timezone: impl Into<String>) -> Self {
        self.timezone = Some(timezone.into());
        self
    }

    /// Inside the KNMI radar grid.
    pub fn in_knmi_bbox(&self) -> bool {
        self.latitude >= KNMI_BBOX_MIN_LAT
            && self.latitude <= KNMI_BBOX_MAX_LAT
            && self.longitude >= KNMI_BBOX_MIN_LON
            && self.longitude <= KNMI_BBOX_MAX_LON
    }

    /// KNMI radar coverage: an NL country code, or (when the code is missing)
    /// coordinates inside the radar grid — same rule as the Android app.
    pub fn in_knmi_coverage(&self) -> bool {
        matches!(self.country_code.as_deref(), Some("NL")) || self.in_knmi_bbox()
    }

    /// Stable cache/serialisation key: coordinates rounded to two decimals.
    pub fn key(&self) -> String {
        format!("{:.2}_{:.2}", self.latitude, self.longitude)
    }

    pub fn slug(&self) -> String {
        self.key().replace(['.', '-'], "_")
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({:.4}, {:.4})", self.name, self.latitude, self.longitude)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_follows_country_code_or_bbox() {
        let leiden = Location::new("Leiden", 52.16, 4.49).with_country_code("nl");
        assert!(leiden.in_knmi_coverage());
        assert!(leiden.in_knmi_bbox());

        // Country code unknown but inside the grid: still covered.
        let nowhere = Location::new("Somewhere", 52.0, 5.0);
        assert!(nowhere.in_knmi_coverage());

        // Berlin and Pune are outside the radar grid.
        let berlin = Location::new("Berlin", 52.52, 13.405).with_country_code("DE");
        assert!(!berlin.in_knmi_coverage());
        let pune = Location::new("Pune", 18.52, 73.85).with_country_code("IN");
        assert!(!pune.in_knmi_coverage());
    }

    #[test]
    fn cache_key_is_stable_and_rounded() {
        let a = Location::new("Leiden", 52.1601, 4.4901);
        let b = Location::new("Leiden", 52.1598, 4.4899);
        assert_eq!(a.key(), b.key());
        assert_eq!(a.key(), "52.16_4.49");
    }
}
