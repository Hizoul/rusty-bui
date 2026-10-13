//! KNMI `radar_forecast_2.0` rain nowcast via WMS `GetFeatureInfo`.
//!
//! 25 steps of 5 minutes covering +0…+120 min for the Netherlands, read from one
//! point query per step. Ported from WeatherMaster's `RainNowcastRepository`:
//! the newest cycle can land late, so the client snaps to the current 5-minute
//! boundary and probes back up to an hour before giving up.

use super::http::HttpClient;
use super::{provider_log, ProviderError, ProviderResult};
use crate::cache::{ResponseCache, RESPONSE_FRESH_SECONDS};
use crate::location::{
    Location, KNMI_BBOX_MAX_LAT, KNMI_BBOX_MAX_LON, KNMI_BBOX_MIN_LAT, KNMI_BBOX_MIN_LON,
};
use crate::nowcast::{
    NowcastPoint, NowcastProvider, RainNowcast, NOWCAST_WINDOW_MINUTES, RADAR_SLOT_MINUTES,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use std::time::Duration as StdDuration;

/// WMS request geometry: full NL bounding box and request raster size.
pub const WMS_BBOX: &str = "3.0,50.5,7.5,54.0";
pub const WMS_WIDTH: u32 = 900;
pub const WMS_HEIGHT: u32 = 700;

const ANONYMOUS_HOST: &str = "https://anonymous.api.dataplatform.knmi.nl/wms/adaguc-server";
const KEYED_HOST: &str = "https://api.dataplatform.knmi.nl/wms/adaguc-server";

/// How far back the newest published cycle is looked for.
const CYCLE_PROBE_MINUTES: i64 = 60;

pub struct KnmiNowcastClient {
    http: HttpClient,
    api_key: Option<String>,
}

impl KnmiNowcastClient {
    pub fn new(api_key: Option<String>, cache: ResponseCache) -> ProviderResult<Self> {
        Ok(Self {
            http: HttpClient::new("KNMI", cache)?,
            api_key: api_key.filter(|key| !key.trim().is_empty()),
        })
    }

    /// Response counters for this client.
    pub fn stats(&self) -> super::FetchStats {
        self.http.stats()
    }

    /// The raw-response cache behind this client.
    pub fn cache(&self) -> &ResponseCache {
        self.http.cache()
    }

    /// Anonymous by default; the keyed host gets a user's own quota.
    pub fn host(&self) -> &'static str {
        if self.api_key.is_some() {
            KEYED_HOST
        } else {
            ANONYMOUS_HOST
        }
    }

    pub fn uses_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// One GetFeatureInfo query for a single 5-minute step.
    fn query(&self, location: &Location, time: DateTime<Utc>) -> Vec<(&'static str, String)> {
        let x = (((location.longitude - KNMI_BBOX_MIN_LON) / (KNMI_BBOX_MAX_LON - KNMI_BBOX_MIN_LON))
            * f64::from(WMS_WIDTH)) as i64;
        let y = (((KNMI_BBOX_MAX_LAT - location.latitude) / (KNMI_BBOX_MAX_LAT - KNMI_BBOX_MIN_LAT))
            * f64::from(WMS_HEIGHT)) as i64;

        vec![
            ("DATASET", "radar_forecast_2.0".to_string()),
            ("SERVICE", "WMS".to_string()),
            ("VERSION", "1.1.1".to_string()),
            ("REQUEST", "GetFeatureInfo".to_string()),
            ("LAYERS", "precipitation_nowcast".to_string()),
            ("QUERY_LAYERS", "precipitation_nowcast".to_string()),
            ("STYLES", "rainrate-blue-to-purple/nearest".to_string()),
            ("SRS", "EPSG:4326".to_string()),
            ("BBOX", WMS_BBOX.to_string()),
            ("WIDTH", WMS_WIDTH.to_string()),
            ("HEIGHT", WMS_HEIGHT.to_string()),
            ("INFO_FORMAT", "text/html".to_string()),
            ("FEATURE_COUNT", "1".to_string()),
            ("X", x.to_string()),
            ("Y", y.to_string()),
            ("TIME", format_wms_time(time)),
        ]
    }

    async fn feature_info(&self, location: &Location, time: DateTime<Utc>) -> ProviderResult<String> {
        // One cache entry per (pixel, 5-minute step): inside a cycle the same point
        // query cannot return anything new, and a forced refresh or a second
        // consumer (bar module, popout, app) then costs nothing.
        let authorization = self.api_key.as_deref().map(|key| ("authorization", key));
        self.http
            .get_text(
                self.host(),
                &self.query(location, time),
                authorization,
                StdDuration::from_secs(RESPONSE_FRESH_SECONDS as u64),
            )
            .await
    }

    pub async fn fetch(
        &self,
        location: &Location,
        now: DateTime<Utc>,
    ) -> ProviderResult<RainNowcast> {
        let snapped = snap_to_radar_cycle(now);

        // A layer only holds exact 5-minute timesteps, and the newest one can be
        // late, so walk back in 5-minute steps until the store answers.
        let mut cycle: Option<DateTime<Utc>> = None;
        let mut back = 0_i64;
        while back <= CYCLE_PROBE_MINUTES && cycle.is_none() {
            let candidate = snapped - Duration::minutes(back);
            match self.feature_info(location, candidate).await {
                Ok(body) if body.contains("forecast") => cycle = Some(candidate),
                Ok(_) => provider_log!("KNMI: cycle {} not published yet", candidate.to_rfc3339()),
                Err(error) => provider_log!("KNMI: probe {back} min back failed: {error}"),
            }
            back += i64::from(RADAR_SLOT_MINUTES);
        }

        let cycle = cycle.ok_or(ProviderError::Unavailable {
            provider: "KNMI",
            reason: "no radar cycle published in the last hour".to_string(),
        })?;

        let steps = NOWCAST_WINDOW_MINUTES / i64::from(RADAR_SLOT_MINUTES);
        let mut points = Vec::with_capacity(steps as usize + 1);
        for step in 0..=steps {
            let time = cycle + Duration::minutes(step * i64::from(RADAR_SLOT_MINUTES));
            match self.feature_info(location, time).await {
                Ok(body) => match parse_gfi_mm_per_hour(&body) {
                    Some(intensity_mm_per_hour) => points.push(NowcastPoint {
                        time,
                        intensity_mm_per_hour,
                        interval_minutes: RADAR_SLOT_MINUTES,
                    }),
                    None => provider_log!("KNMI: no intensity at step +{}", step * 5),
                },
                Err(error) => provider_log!("KNMI: step +{} failed: {error}", step * 5),
            }
        }

        if points.is_empty() {
            return Err(ProviderError::Unavailable {
                provider: "KNMI",
                reason: "every step of the cycle came back empty".to_string(),
            });
        }

        Ok(RainNowcast::new(points, NowcastProvider::Knmi, true))
    }
}

/// Floor to the 5-minute boundary the radar layers use, dropping sub-second
/// precision (which makes the server answer "store has no results").
pub fn snap_to_radar_cycle(now: DateTime<Utc>) -> DateTime<Utc> {
    let minute = i64::from(now.minute());
    let floored_minute = minute - minute % i64::from(RADAR_SLOT_MINUTES);
    now.with_minute(floored_minute as u32)
        .and_then(|time| time.with_second(0))
        .and_then(|time| time.with_nanosecond(0))
        .unwrap_or(now)
}

pub fn format_wms_time(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// The GetFeatureInfo body is a tiny HTML table:
/// `<td>forecast</td><td><b>0.12</b></td><td> mm/hr</td>`.
pub fn parse_gfi_mm_per_hour(body: &str) -> Option<f64> {
    let text = strip_tags(body);
    let mut search_from = 0_usize;

    while let Some(offset) = text[search_from..].find("forecast") {
        let after_keyword = search_from + offset + "forecast".len();
        let tail = text[after_keyword..].trim_start();

        let digits: String = tail
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();

        if !digits.is_empty() {
            if let Ok(value) = digits.parse::<f64>() {
                if tail[digits.len()..].trim_start().starts_with("mm/hr") {
                    return Some(value.max(0.0));
                }
            }
        }

        search_from = after_keyword;
    }

    None
}

/// Every tag becomes a separating space, matching the original's
/// `replace(Regex("<[^>]*>"), " ")`.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if in_tag => {}
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Verbatim response captured from the anonymous KNMI WMS host.
    const LIVE_SAMPLE: &str = "<html>Pointer Coordinates: (lon=4.49; lat=52.16)<br><hr/>\
<b>forecast (precipitation_nowcast)</b><br/><table><tr><td>&nbsp;</td><td>forecast</td>\
<td><b>0</b></td><td> mm/hr</td></tr></table></html>";

    #[test]
    fn parses_the_live_sample() {
        assert_eq!(parse_gfi_mm_per_hour(LIVE_SAMPLE), Some(0.0));
    }

    #[test]
    fn parses_decimals_and_ignores_the_heading_when_scanning() {
        let body = "<b>forecast (precipitation_nowcast)</b><td>forecast</td><td><b>2.35</b></td><td> mm/hr</td>";
        assert_eq!(parse_gfi_mm_per_hour(body), Some(2.35));
    }

    #[test]
    fn rejects_a_store_exception() {
        let body = "<ServiceExceptionReport><ServiceException>Store has no results. \
Invalid dimension value for layer precipitation_nowcast</ServiceException></ServiceExceptionReport>";
        assert_eq!(parse_gfi_mm_per_hour(body), None);
    }

    #[test]
    fn never_returns_negative_intensity() {
        let body = "<td>forecast</td><td><b>-0.5</b></td><td> mm/hr</td>";
        assert_eq!(parse_gfi_mm_per_hour(body), None);
    }

    #[test]
    fn snapping_drops_seconds_and_snaps_to_five_minutes() {
        let now = Utc.with_ymd_and_hms(2026, 10, 11, 15, 43, 59).unwrap();
        let snapped = snap_to_radar_cycle(now);
        assert_eq!(snapped.minute(), 40);
        assert_eq!(snapped.second(), 0);
        assert_eq!(format_wms_time(snapped), "2026-10-11T15:40:00Z");

        let on_boundary = Utc.with_ymd_and_hms(2026, 10, 11, 15, 40, 12).unwrap();
        assert_eq!(snap_to_radar_cycle(on_boundary).minute(), 40);
    }

    #[test]
    fn point_query_pixels_stay_inside_the_raster() {
        let client = KnmiNowcastClient::new(None, ResponseCache::disabled()).unwrap();
        let leiden = Location::new("Leiden", 52.16, 4.49);
        let query = client.query(&leiden, Utc.with_ymd_and_hms(2026, 10, 11, 15, 40, 0).unwrap());
        let get = |key: &str| {
            query.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone()).unwrap()
        };
        let x: i64 = get("X").parse().unwrap();
        let y: i64 = get("Y").parse().unwrap();
        assert!(x > 0 && x < i64::from(WMS_WIDTH), "x out of raster: {x}");
        assert!(y > 0 && y < i64::from(WMS_HEIGHT), "y out of raster: {y}");
        assert_eq!(get("BBOX"), WMS_BBOX);
        assert!(!client.uses_api_key());
    }
}
