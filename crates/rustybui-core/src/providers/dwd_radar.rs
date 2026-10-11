//! DWD `Radarkomposit RV` rain nowcast via Bright Sky's point-extraction
//! endpoint. 5-minute frames covering Germany, observed plus extrapolated.
//!
//! Each frame carries a base64/zlib int16 little-endian grid (0.01 mm per 5
//! minutes per unit) cropped around the requested point, plus the pixel position
//! of that point inside the crop. Ported from WeatherMaster's `RainNowcastRepository`.

use super::http::HttpClient;
use super::{provider_log, ProviderError, ProviderResult};
use crate::cache::{snap_to_cycle, ResponseCache, RESPONSE_FRESH_SECONDS};
use crate::location::Location;
use crate::nowcast::{
    NowcastPoint, NowcastProvider, RainNowcast, NOWCAST_PAST_CONTEXT_MINUTES,
    NOWCAST_WINDOW_MINUTES, RADAR_SLOT_MINUTES,
};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use flate2::read::ZlibDecoder;
use serde::Deserialize;
use std::io::Read;
use std::time::Duration as StdDuration;

const BASE_URL: &str = "https://api.brightsky.dev/radar";

/// Half-size of the crop window requested around the point, in degrees.
const CROP_HALF_DEGREES: f64 = 0.02;

/// Raw grid units are 0.01 mm per 5 minutes, so units × 0.12 is mm/h.
const UNIT_TO_MM_PER_HOUR: f64 = 0.01 * 12.0;

#[derive(Debug, Deserialize)]
struct RadarResponse {
    radar: Option<Vec<RadarFrame>>,
    /// `[cropX, cropY, cropX + width, cropY + height]` in composite pixels.
    bbox: Option<Vec<i64>>,
    latlon_position: Option<LatLonPosition>,
}

#[derive(Debug, Deserialize)]
struct RadarFrame {
    timestamp: Option<String>,
    #[serde(rename = "precipitation_5")]
    precipitation_5: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LatLonPosition {
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
}

pub struct DwdRadarClient {
    http: HttpClient,
}

impl DwdRadarClient {
    pub fn new(cache: ResponseCache) -> ProviderResult<Self> {
        Ok(Self { http: HttpClient::new("DWD", cache)? })
    }

    /// Response counters for this client.
    pub fn stats(&self) -> super::FetchStats {
        self.http.stats()
    }

    pub async fn fetch(
        &self,
        location: &Location,
        now: DateTime<Utc>,
    ) -> ProviderResult<RainNowcast> {
        // Bright Sky takes the request date as the day/first-frame selector and
        // returns the composite's own ±2 h window, so snapping `date` to the radar
        // cycle keeps the URL — and therefore the cache key — stable for 5 minutes.
        let start = snap_to_cycle(now - Duration::minutes(NOWCAST_PAST_CONTEXT_MINUTES));
        let query = vec![
            ("lat", location.latitude.to_string()),
            ("lon", location.longitude.to_string()),
            ("date", start.format("%Y-%m-%dT%H:%M:00+00:00").to_string()),
            ("max_lat", (location.latitude + CROP_HALF_DEGREES).to_string()),
            ("min_lat", (location.latitude - CROP_HALF_DEGREES).to_string()),
            ("max_lon", (location.longitude + CROP_HALF_DEGREES).to_string()),
            ("min_lon", (location.longitude - CROP_HALF_DEGREES).to_string()),
        ];

        let body = self
            .http
            .get_text(
                BASE_URL,
                &query,
                None,
                StdDuration::from_secs(RESPONSE_FRESH_SECONDS as u64),
            )
            .await?;

        let body: RadarResponse = serde_json::from_str(&body).map_err(|error| {
            ProviderError::Decode {
                provider: "DWD",
                reason: format!("invalid radar JSON: {error}"),
            }
        })?;

        let bbox = body.bbox.ok_or_else(|| ProviderError::Decode {
            provider: "DWD",
            reason: "radar response has no bbox".to_string(),
        })?;
        if bbox.len() < 4 {
            return Err(ProviderError::Decode {
                provider: "DWD",
                reason: format!("radar bbox has {} entries, expected 4", bbox.len()),
            });
        }

        let position = body.latlon_position.ok_or_else(|| ProviderError::Decode {
            provider: "DWD",
            reason: "radar response has no latlon_position".to_string(),
        })?;
        let (ix, iy) = match (position.x, position.y) {
            (Some(x), Some(y)) => (x.round() as i64, y.round() as i64),
            _ => {
                return Err(ProviderError::Decode {
                    provider: "DWD",
                    reason: "latlon_position is missing x/y".to_string(),
                })
            }
        };

        // The crop width comes from the bbox; `latlon_position` is relative to
        // the crop origin.
        let width = bbox[2] - bbox[0];
        let window_start = now - Duration::minutes(NOWCAST_PAST_CONTEXT_MINUTES);
        let window_end = now + Duration::minutes(NOWCAST_WINDOW_MINUTES);

        let mut points = Vec::new();
        for frame in body.radar.unwrap_or_default() {
            let (Some(encoded), Some(timestamp)) = (frame.precipitation_5, frame.timestamp) else {
                continue;
            };
            let Ok(time) = DateTime::parse_from_rfc3339(&timestamp) else {
                provider_log!("DWD: unparseable frame timestamp {timestamp:?}");
                continue;
            };
            let time = time.with_timezone(&Utc);

            // Frames reach further back and forward than the chart window.
            if time < window_start || time > window_end {
                continue;
            }

            match grid_value(&encoded, width, ix, iy) {
                Some(raw) => points.push(NowcastPoint {
                    time,
                    intensity_mm_per_hour: raw * UNIT_TO_MM_PER_HOUR,
                    interval_minutes: RADAR_SLOT_MINUTES,
                }),
                None => provider_log!("DWD: frame {timestamp} has no value at ({ix}, {iy})"),
            }
        }

        if points.is_empty() {
            return Err(ProviderError::Unavailable {
                provider: "DWD",
                reason: "no radar frame covered the requested point inside the window".to_string(),
            });
        }

        Ok(RainNowcast::new(points, NowcastProvider::Dwd, true))
    }
}

/// Decompresses one frame (base64 + zlib int16 little-endian grid) and returns
/// the unsigned value at grid position ([ix], [iy]).
pub fn grid_value(encoded: &str, width: i64, ix: i64, iy: i64) -> Option<f64> {
    if width <= 0 || ix < 0 || iy < 0 || ix >= width {
        return None;
    }

    let compressed = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut inflated = Vec::new();
    decoder.read_to_end(&mut inflated).ok()?;

    let count = inflated.len() / 2;
    let index = iy * width + ix;
    if count == 0 || index < 0 || index as usize >= count {
        return None;
    }

    let low = u16::from(inflated[index as usize * 2]);
    let high = u16::from(inflated[index as usize * 2 + 1]);
    Some(f64::from((high << 8) | low))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    /// Encodes a grid the way Bright Sky does: zlib-compressed int16 LE, base64.
    fn encode_grid(values: &[i16]) -> String {
        let mut bytes = Vec::with_capacity(values.len() * 2);
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&bytes).unwrap();
        let compressed = encoder.finish().unwrap();
        base64::engine::general_purpose::STANDARD.encode(compressed)
    }

    #[test]
    fn reads_the_value_at_a_grid_position() {
        // 4 x 3 grid, row-major, values 0..11.
        let grid: Vec<i16> = (0..12).collect();
        let encoded = encode_grid(&grid);

        assert_eq!(grid_value(&encoded, 4, 0, 0), Some(0.0));
        assert_eq!(grid_value(&encoded, 4, 3, 0), Some(3.0));
        assert_eq!(grid_value(&encoded, 4, 2, 1), Some(6.0)); // index 1*4 + 2
        assert_eq!(grid_value(&encoded, 4, 0, 2), Some(8.0));
    }

    #[test]
    fn reads_values_above_the_signed_range_as_unsigned() {
        // DWD grids are unsigned; 0xFFFE must decode as 65534, not -2.
        let encoded = encode_grid(&[0, -2]);
        assert_eq!(grid_value(&encoded, 2, 1, 0), Some(65534.0));
    }

    #[test]
    fn rejects_out_of_range_positions_and_garbage() {
        let encoded = encode_grid(&(0..12).collect::<Vec<i16>>());
        assert_eq!(grid_value(&encoded, 4, 4, 0), None); // outside the row
        assert_eq!(grid_value(&encoded, 4, 0, 3), None); // past the last row
        assert_eq!(grid_value(&encoded, 4, -1, 0), None);
        assert_eq!(grid_value(&encoded, 0, 0, 0), None);
        assert_eq!(grid_value("not base64!", 4, 0, 0), None);
        assert_eq!(grid_value(&encode_grid(&[]), 1, 0, 0), None);
    }

    #[test]
    fn converts_raw_units_to_mm_per_hour() {
        // 25 units = 0.25 mm per 5 min = 3 mm/h.
        assert!((25.0 * UNIT_TO_MM_PER_HOUR - 3.0).abs() < 1e-9);
    }
}
