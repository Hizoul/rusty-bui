//! Radar imagery for the map.
//!
//! DWD's composite, through the public GeoServer WMS that publishes it. A WMS answers in the
//! projection it was asked for, so an EPSG:4326 bounding box comes back as a lat/lon rectangle
//! and the image needs no georeferencing of its own — which is what makes this drawable over a
//! map, unlike the point-sampled grids the nowcast reads.
//!
//! The service is not always well: it answers 500 or hangs for a minute at a time. So requests
//! are retried, each image is cached in a directory of its own keyed by box, size and frame, and
//! a failure leaves the caller showing the last image it has rather than nothing at all.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};

use crate::providers::http::HttpClient;
use crate::providers::{provider_log, ProviderError, ProviderResult};

const WMS_URL: &str = "https://maps.dwd.de/geoserver/dwd/wms";
const LAYER: &str = "dwd:Niederschlagsradar";

/// CC BY 4.0, like the rest of DWD's open data.
pub const RADAR_ATTRIBUTION: &str = "Radar © DWD";

/// How long an image stays good. The composite is republished every five minutes and the frame
/// is part of the cache key, so this only has to cover being asked again in the same cycle.
const IMAGE_FRESH: Duration = Duration::from_secs(300);

/// The frame list changes at most hourly.
const CAPABILITIES_FRESH: Duration = Duration::from_secs(3600);

const ATTEMPTS: usize = 3;

/// The newest frames kept, which is also what a stepper can walk back through.
pub const FRAME_LIMIT: usize = 24;

pub struct RadarMapClient {
    http: HttpClient,
    dir: PathBuf,
}

/// One radar image, on disk and georeferenced by its bounding box.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadarImage {
    /// Where the PNG is; a front-end loads it as a local file.
    pub path: PathBuf,
    /// `[minLon, minLat, maxLon, maxLat]` — what the image covers.
    pub bbox: [f64; 4],
    pub width: u32,
    pub height: u32,
    /// The frame it shows, epoch milliseconds.
    pub time_ms: i64,
    pub attribution: String,
    /// Served from disk rather than fetched.
    pub cached: bool,
}

impl RadarMapClient {
    pub fn new(http: HttpClient, dir: PathBuf) -> Self {
        Self { http, dir }
    }

    /// The image for `bbox` at `width`×`height`, for `time` (or the newest frame when none
    /// is asked for).
    pub async fn image(
        &self,
        bbox: [f64; 4],
        width: u32,
        height: u32,
        time: Option<DateTime<Utc>>,
    ) -> ProviderResult<RadarImage> {
        let frame = match time {
            Some(frame) => Some(frame),
            // The extent runs into the future — it is a nowcast — so the map opens on the frame
            // nearest now rather than on the last one, which would show rain still to arrive.
            None => self.frames().await.ok().and_then(|frames| closest_to_now(&frames)),
        };

        let path = self.path_for(bbox, width, height, frame);
        let stamp = frame.unwrap_or_else(Utc::now);
        let attribution = RADAR_ATTRIBUTION.to_string();
        // Offline, any image on disk will do: it is a local file, and a radar picture from an
        // hour ago beats an empty map.
        let offline = self.http.cache().is_offline();

        if let Some(cached) = self.cached_image(&path, offline) {
            provider_log!("radar: cache hit for {}", path.display());
            return Ok(RadarImage {
                path: cached,
                bbox,
                width,
                height,
                time_ms: stamp.timestamp_millis(),
                attribution,
                cached: true,
            });
        }

        let query = vec![
            ("SERVICE", "WMS".to_string()),
            ("VERSION", "1.1.1".to_string()),
            ("REQUEST", "GetMap".to_string()),
            ("LAYERS", LAYER.to_string()),
            ("STYLES", String::new()),
            ("SRS", "EPSG:4326".to_string()),
            (
                "BBOX",
                format!("{},{},{},{}", bbox[0], bbox[1], bbox[2], bbox[3]),
            ),
            ("WIDTH", width.to_string()),
            ("HEIGHT", height.to_string()),
            ("FORMAT", "image/png".to_string()),
            // Everything outside the composite's own extent stays clear, so the map underneath
            // shows through.
            ("TRANSPARENT", "TRUE".to_string()),
        ];
        let query: Vec<(&str, String)> = match frame {
            Some(frame) => query
                .into_iter()
                .chain([(
                    "TIME",
                    frame.format("%Y-%m-%dT%H:%M:%S.000Z").to_string(),
                )])
                .collect(),
            None => query,
        };

        let bytes = self.fetch_with_retries(&query).await?;
        if !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            return Err(ProviderError::Decode {
                provider: "DWD",
                reason: "the map response was not a PNG".to_string(),
            });
        }

        write_atomically(&path, &bytes)?;

        Ok(RadarImage {
            path,
            bbox,
            width,
            height,
            time_ms: stamp.timestamp_millis(),
            attribution,
            cached: false,
        })
    }

    /// The frames the composite has, oldest first, at most [`FRAME_LIMIT`].
    ///
    /// Read from the layer's time dimension in `GetCapabilities`, which is either a list or a
    /// start/end/period triple.
    pub async fn frames(&self) -> ProviderResult<Vec<DateTime<Utc>>> {
        let capabilities = self
            .http
            .get_text(
                WMS_URL,
                &[
                    ("SERVICE", "WMS".to_string()),
                    ("VERSION", "1.1.1".to_string()),
                    ("REQUEST", "GetCapabilities".to_string()),
                ],
                None,
                CAPABILITIES_FRESH,
            )
            .await?;

        let mut frames = parse_frames(&capabilities).map_err(|reason| ProviderError::Decode {
            provider: "DWD",
            reason,
        })?;
        if frames.len() > FRAME_LIMIT {
            frames.drain(..frames.len() - FRAME_LIMIT);
        }
        Ok(frames)
    }

    /// The service is flaky enough that one attempt is not a fair try.
    async fn fetch_with_retries(&self, query: &[(&str, String)]) -> ProviderResult<Vec<u8>> {
        let mut last = None;
        for attempt in 1..=ATTEMPTS {
            match self.http.get_bytes(WMS_URL, query, None).await {
                Ok(bytes) => return Ok(bytes),
                Err(error) => {
                    provider_log!("radar: attempt {attempt}/{ATTEMPTS} failed: {error}");
                    last = Some(error);
                    if attempt < ATTEMPTS {
                        tokio::time::sleep(Duration::from_millis(1500 * attempt as u64)).await;
                    }
                }
            }
        }
        Err(last.unwrap_or(ProviderError::Unavailable {
            provider: "DWD",
            reason: "no attempt was made".to_string(),
        }))
    }

    /// A cached image for the same box, size and frame. Inside the freshness window while
    /// online; anything on disk at all when there is no network to ask.
    fn cached_image(&self, path: &Path, offline: bool) -> Option<PathBuf> {
        let metadata = std::fs::metadata(path).ok()?;
        if offline {
            return Some(path.to_path_buf());
        }
        let age = metadata.modified().ok()?.elapsed().ok()?;
        (age < IMAGE_FRESH).then(|| path.to_path_buf())
    }

    /// One file per box, size and frame, so a pan or a step back lands on the same file again.
    fn path_for(
        &self,
        bbox: [f64; 4],
        width: u32,
        height: u32,
        frame: Option<DateTime<Utc>>,
    ) -> PathBuf {
        let stamp = frame.map_or_else(|| "latest".to_string(), |frame| frame.timestamp().to_string());
        let name = format!(
            "{:.3}_{:.3}_{:.3}_{:.3}_{width}x{height}_{stamp}.png",
            bbox[0], bbox[1], bbox[2], bbox[3]
        );
        self.dir.join(name)
    }
}

/// Writes through a temporary file, so a reader never sees half an image.
fn write_atomically(path: &Path, bytes: &[u8]) -> ProviderResult<()> {
    if path.parent().is_none() {
        return Err(ProviderError::Unavailable {
            provider: "DWD",
            reason: "the map cache path has no directory".to_string(),
        });
    }
    crate::private_file::write_private(path, bytes).map_err(|error| ProviderError::Unavailable {
        provider: "DWD",
        reason: format!("writing {}: {error}", path.display()),
    })
}

/// Pulls the frames out of a `GetCapabilities` document.
///
/// Deliberately string surgery rather than an XML parser: the document is 1.8 MB, of which the
/// radar layer's time extent is a slice we want, and that element's shape is fixed — a
/// `<Dimension>` that is usually empty, next to an `<Extent>` that carries the frames.
///
/// The extent lists every minute the store holds, days deep, while the composite itself is
/// republished every five: the list is thinned to the radar's own cadence, newest kept, so a
/// stepper walks radar cycles rather than repeats of the same picture.
///
/// Thinned by *distance* rather than by alignment: the composite runs at its own phase (five
/// minutes apart, but not on clock multiples of five), so anything that assumed :00/:05/:10
/// would throw away every frame.
pub fn parse_frames(capabilities: &str) -> Result<Vec<DateTime<Utc>>, String> {
    let layer = capabilities
        .find(&format!("<Name>{LAYER}</Name>"))
        .or_else(|| capabilities.find("<Name>Niederschlagsradar</Name>"))
        .ok_or_else(|| "the radar layer is not in the capabilities document".to_string())?;
    let rest = &capabilities[layer..];
    // The layer's own block. These documents nest layers, so it ends at the next layer's name
    // rather than at the first closing tag.
    let end = rest[1..].find("<Name>").map_or(rest.len(), |offset| offset + 1);
    let block = &rest[..end];

    let value = value_of(block, "Extent")
        .or_else(|| value_of(block, "Dimension"))
        .ok_or_else(|| "the radar layer has no time extent".to_string())?;

    // Either a `start/end/period` triple or a list of instants; both shapes turn up, and DWD
    // states the radar layer as a range.
    let frames = if value.contains('/') {
        expand_range(value)?
    } else {
        parse_frame_list(value)
    };

    let mut frames: Vec<DateTime<Utc>> = frames;
    frames.sort_unstable();
    frames.dedup();

    let mut thinned: Vec<DateTime<Utc>> = Vec::new();
    for frame in frames.into_iter().rev() {
        match thinned.last() {
            Some(kept) if (kept.timestamp() - frame.timestamp()).abs() < 240 => continue,
            _ => thinned.push(frame),
        }
    }
    thinned.reverse();

    if thinned.is_empty() {
        return Err("the time extent carried no frames".to_string());
    }
    Ok(thinned)
}

/// The frame nearest now. The extent a WMS publishes for a nowcast runs past the present, so
/// "the newest frame" is a forecast.
pub fn closest_to_now(frames: &[DateTime<Utc>]) -> Option<DateTime<Utc>> {
    let now = Utc::now().timestamp();
    frames
        .iter()
        .copied()
        .min_by_key(|frame| (frame.timestamp() - now).abs())
}

/// The text of a `name="time"` element, when it is present and not self-closing.
fn value_of<'a>(block: &'a str, element: &str) -> Option<&'a str> {
    let start = block.find(&format!("<{element} name=\"time\""))?;
    let after = &block[start..];
    let open = after.find('>')?;
    if after[..open].ends_with('/') {
        return None;
    }
    let close = after.find(&format!("</{element}>"))?;
    Some(&after[open + 1..close])
}

fn parse_frame_list(value: &str) -> Vec<DateTime<Utc>> {
    value.split(',').filter_map(|part| parse_frame(part.trim())).collect()
}

/// Expands `start/end/period`, which is how GeoServer usually states a radar layer's frames.
fn expand_range(value: &str) -> Result<Vec<DateTime<Utc>>, String> {
    let parts: Vec<&str> = value.split('/').collect();
    if parts.len() != 3 {
        return Err(format!("unrecognised time dimension shape: {value}"));
    }
    let start = parse_frame(parts[0]).ok_or_else(|| format!("bad start frame: {}", parts[0]))?;
    let end = parse_frame(parts[1]).ok_or_else(|| format!("bad end frame: {}", parts[1]))?;
    let step = parse_period(parts[2]).ok_or_else(|| format!("bad period: {}", parts[2]))?;

    let mut frames = Vec::new();
    let mut frame = start;
    // A guard against a period that rounds to nothing, which would otherwise loop for ever.
    while frame <= end && frames.len() < 10_000 {
        frames.push(frame);
        frame += step;
    }
    Ok(frames)
}

/// `PT5M`, `PT1H`, `PT1H30M`, `P1D` — the shapes a WMS states radar frames in.
fn parse_hours_minutes(value: &str) -> Option<ChronoDuration> {
    let body = value.trim().strip_prefix('P')?;
    let (days, time) = match body.split_once('D') {
        Some((days, rest)) => (days.parse::<i64>().ok()?, rest),
        None => (0, body),
    };
    let time = time.strip_prefix('T').unwrap_or("");
    let (hours, minutes) = match time.split_once('H') {
        Some((hours, rest)) => (hours.parse::<i64>().ok()?, rest),
        None => (0, time),
    };
    let minutes = if minutes.is_empty() {
        0
    } else {
        minutes.strip_suffix('M').unwrap_or(minutes).parse::<i64>().ok()?
    };

    let total = days * 24 * 60 + hours * 60 + minutes;
    (total > 0).then(|| ChronoDuration::minutes(total))
}

fn parse_period(value: &str) -> Option<ChronoDuration> {
    parse_hours_minutes(value.trim())
}

fn parse_frame(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.trim())
        .ok()
        .map(|frame| frame.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::ResponseCache;

    fn client() -> RadarMapClient {
        let cache = ResponseCache::at(std::env::temp_dir().join("rustybui-radar-unit-test"));
        RadarMapClient::new(HttpClient::new("DWD", cache).unwrap(), PathBuf::from("/tmp/radar"))
    }

    const CAPABILITIES: &str = r#"<?xml version="1.0"?>
<WMS_Capabilities version="1.1.1">
  <Capability><Layer><Title>DWD</Title>
    <Layer queryable="1">
      <Name>dwd:Niederschlagsradar</Name>
      <Title>Niederschlagsradar</Title>
      <SRS>EPSG:4326</SRS>
      <LatLonBoundingBox minx="1.46" miny="45.68" maxx="18.73" maxy="56.22"/>
      <Dimension name="time" units="ISO8601"/>
      <Extent name="time" default="current" nearestValue="0">2026-10-11T16:18:00.000Z,2026-10-11T16:19:00.000Z,2026-10-11T16:20:00.000Z,2026-10-11T16:21:00.000Z,2026-10-11T16:22:00.000Z,2026-10-11T16:23:00.000Z,2026-10-11T16:24:00.000Z,2026-10-11T16:25:00.000Z</Extent>
    </Layer>
    <Layer queryable="1">
      <Name>Warngebiete_Grenzen</Name>
      <Dimension name="time" units="ISO8601">2026-01-01T00:00:00.000Z/2026-01-02T00:00:00.000Z/PT1H</Dimension>
    </Layer>
  </Layer></Capability>
</WMS_Capabilities>"#;

    #[test]
    fn reads_the_frame_extent_a_geoserver_states() {
        let frames = parse_frames(CAPABILITIES).unwrap();
        // The store lists every minute; the list is thinned to the radar's own cadence.
        assert_eq!(frames.len(), 2);
        assert_eq!(
            frames[0],
            DateTime::parse_from_rfc3339("2026-10-11T16:21:00Z").unwrap().with_timezone(&Utc)
        );
        assert_eq!(
            frames.last().unwrap(),
            &DateTime::parse_from_rfc3339("2026-10-11T16:25:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn falls_back_to_a_range_on_the_dimension() {
        let document = r#"<Layer><Name>Niederschlagsradar</Name>
            <Dimension name="time">2026-10-11T16:20:00.000Z/2026-10-11T16:40:00.000Z/PT5M</Dimension>
            </Layer>"#;
        let frames = parse_frames(document).unwrap();
        assert_eq!(frames.len(), 5);
        assert!(frames[0] < frames[4]);
    }

    #[test]
    fn reads_a_comma_separated_dimension_too() {
        let document = r#"<Layer><Name>Niederschlagsradar</Name>
            <Dimension name="time">2026-10-11T17:00:00.000Z,2026-10-11T17:05:00.000Z</Dimension>
            </Layer>"#;
        let frames = parse_frames(document).unwrap();
        assert_eq!(frames.len(), 2);
        assert!(frames[0] < frames[1]);
    }

    #[test]
    fn refuses_a_document_without_the_layer() {
        assert!(parse_frames("<WMS_Capabilities></WMS_Capabilities>").is_err());
        // A layer with no time at all, rather than picking up the next layer's extent.
        assert!(parse_frames("<Layer><Name>Niederschlagsradar</Name></Layer>").is_err());
        assert!(
            parse_frames(
                "<Layer><Name>Niederschlagsradar</Name></Layer>\
                 <Layer><Name>Other</Name><Dimension name=\"time\">2026-10-11T17:00:00Z</Dimension></Layer>"
            )
            .is_err()
        );
    }

    #[test]
    fn reads_a_range_stated_on_the_extent() {
        // What DWD's radar layer actually publishes: a range that runs into the future.
        let document = r#"<Layer><Name>Niederschlagsradar</Name>
            <Dimension name="time" units="ISO8601"/>
            <Extent name="time" default="current">2026-10-11T16:00:00.000Z/2026-10-11T18:00:00.000Z/PT5M</Extent>
            </Layer>"#;
        let frames = parse_frames(document).unwrap();
        assert_eq!(frames.len(), 25);
        assert!(frames[0] < frames[24]);
        assert_eq!(frames[24].timestamp() - frames[23].timestamp(), 300);
    }

    #[test]
    fn the_map_opens_on_now_rather_than_on_the_forecast() {
        let frames: Vec<DateTime<Utc>> = (0..5)
            .map(|step| Utc::now() + ChronoDuration::minutes(5 * step - 10))
            .collect();
        let nearest = closest_to_now(&frames).unwrap();
        // Five frames, ten minutes apart in total: the one nearest the present, not the last.
        assert_ne!(nearest, *frames.last().unwrap());
        assert!((nearest.timestamp() - Utc::now().timestamp()).abs() <= 300);
        assert_eq!(closest_to_now(&[]), None);
    }

    #[test]
    fn knows_the_period_shapes_a_wms_uses() {
        assert_eq!(parse_period("PT5M"), Some(ChronoDuration::minutes(5)));
        assert_eq!(parse_period("PT1H"), Some(ChronoDuration::minutes(60)));
        assert_eq!(parse_period("PT1H30M"), Some(ChronoDuration::minutes(90)));
        assert_eq!(parse_period("P1D"), Some(ChronoDuration::minutes(1440)));
        assert_eq!(parse_period("PT0M"), None);
        assert_eq!(parse_period("nonsense"), None);
    }

    #[test]
    fn an_image_is_named_by_what_it_shows() {
        let client = client();
        let box_ = [4.0, 51.6, 6.0, 52.7];
        let frame = DateTime::parse_from_rfc3339("2026-10-11T17:20:00Z").unwrap().with_timezone(&Utc);

        let name = client.path_for(box_, 800, 440, Some(frame));
        assert_eq!(name.file_name().unwrap().to_str().unwrap(), "4.000_51.600_6.000_52.700_800x440_1791739200.png");
        // Same frame, same file; a different frame, a different file; and "latest" is its own.
        assert_eq!(client.path_for(box_, 800, 440, Some(frame)), name);
        assert_ne!(client.path_for(box_, 800, 440, None), name);
        assert_ne!(client.path_for(box_, 400, 220, Some(frame)), name);
    }
}
