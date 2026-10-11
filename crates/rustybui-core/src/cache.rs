//! Caching layers, all on disk under `$XDG_CACHE_HOME/rustybui`.
//!
//! Three of them, deliberately separate:
//!
//! * [`DiskCache`] — a finished nowcast, keyed per radar cycle.
//! * [`ResponseCache`] — raw provider responses, keyed per request URL, with a
//!   freshness window and a stale fallback.
//! * the request gate in [`crate::providers::http`] — a minimum gap and a
//!   per-minute budget per provider.
//!
//! The endpoints behind this tool are free and run on somebody else's hardware:
//! KNMI answers 24 point queries per nowcast, Bright Sky ships about a megabyte
//! of grids, and Open-Meteo publishes rate limits. Nothing here should re-request
//! data that cannot have changed, and a burst must never leave the machine. Radar
//! data is republished on 5-minute boundaries, so that is the natural window.

use crate::location::Location;
use crate::nowcast::RainNowcast;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Radar frames land on 5-minute boundaries.
pub const NOWCAST_CYCLE_SECONDS: i64 = 300;

/// How long a raw provider response stays fresh.
pub const RESPONSE_FRESH_SECONDS: i64 = NOWCAST_CYCLE_SECONDS;

/// How long a cached response may still be served when the network is down or a
/// request fails: long enough to survive an outage, short enough to be worth
/// showing.
pub const RESPONSE_MAX_STALE_SECONDS: i64 = 6 * 60 * 60;

/// Start of the cycle `now` falls in, as a Unix timestamp.
pub fn cycle_start(now: DateTime<Utc>) -> i64 {
    now.timestamp() - now.timestamp().rem_euclid(NOWCAST_CYCLE_SECONDS)
}

pub fn cycle_end(cycle: i64) -> i64 {
    cycle + NOWCAST_CYCLE_SECONDS
}

/// `now` floored to the radar cycle it sits in.
pub fn snap_to_cycle(now: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp(cycle_start(now), 0).unwrap_or(now)
}

fn cache_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("rustybui"));
        }
    }
    let home = std::env::var_os("HOME")?;
    (!home.is_empty()).then(|| PathBuf::from(home).join(".cache").join("rustybui"))
}

// ---------------------------------------------------------------------------
// Finished nowcasts, keyed per cycle
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct CycleEntry {
    cycle: i64,
    nowcast: RainNowcast,
}

/// JSON files under `<cache root>/nowcast`.
#[derive(Debug, Clone)]
pub struct DiskCache {
    dir: PathBuf,
    enabled: bool,
}

impl DiskCache {
    /// Cache in the user's cache directory. Falls back to a disabled cache when
    /// no home/cache directory can be determined.
    pub fn new() -> Self {
        match cache_root() {
            Some(dir) => Self::at(dir.join("nowcast")),
            None => Self::disabled(),
        }
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), enabled: true }
    }

    /// A cache that never stores or returns anything (used by tests).
    pub fn disabled() -> Self {
        Self { dir: PathBuf::new(), enabled: false }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, location: &Location) -> PathBuf {
        self.dir.join(format!("{}.json", location.slug()))
    }

    /// A cached nowcast for this location, when it belongs to `cycle` — or to any
    /// later cycle, which happens after a clock jump.
    pub fn load_nowcast(&self, location: &Location, cycle: i64) -> Option<RainNowcast> {
        if !self.enabled {
            return None;
        }
        let raw = std::fs::read_to_string(self.path_for(location)).ok()?;
        let entry: CycleEntry = serde_json::from_str(&raw).ok()?;
        (entry.cycle >= cycle && !entry.nowcast.is_empty()).then_some(entry.nowcast)
    }

    pub fn store_nowcast(&self, location: &Location, cycle: i64, nowcast: &RainNowcast) {
        if !self.enabled || nowcast.is_empty() {
            return;
        }
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let entry = CycleEntry { cycle, nowcast: nowcast.clone() };
        if let Ok(raw) = serde_json::to_string(&entry) {
            // Best-effort: a failed cache write must never break a fetch.
            let _ = std::fs::write(self.path_for(location), raw);
        }
    }

    /// Drops the cached nowcast for one location.
    pub fn invalidate(&self, location: &Location) {
        if self.enabled {
            let _ = std::fs::remove_file(self.path_for(location));
        }
    }

    /// Removes every cached nowcast.
    pub fn clear(&self) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        match std::fs::remove_dir_all(&self.dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Default for DiskCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Raw provider responses, keyed per request URL
// ---------------------------------------------------------------------------

/// One cached response on disk. Visible to the crate so tests (and only tests)
/// can age an entry instead of sleeping.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CachedResponse {
    /// Kept so a cache file can be inspected by hand; the filename is a hash.
    pub(crate) url: String,
    pub(crate) fetched_at: i64,
    pub(crate) body: String,
}

/// Raw response cache with a freshness window, a stale fallback and an offline
/// mode. Keyed by a deterministic hash of the full request URL.
#[derive(Debug, Clone)]
pub struct ResponseCache {
    dir: PathBuf,
    enabled: bool,
    offline: bool,
}

impl ResponseCache {
    pub fn new() -> Self {
        match cache_root() {
            Some(dir) => Self::at(dir.join("http")),
            None => Self::disabled(),
        }
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), enabled: true, offline: false }
    }

    pub fn disabled() -> Self {
        Self { dir: PathBuf::new(), enabled: false, offline: false }
    }

    /// Serve whatever is on disk, however old, and never touch the network.
    pub fn offline(mut self) -> Self {
        self.offline = true;
        self
    }

    pub fn is_offline(&self) -> bool {
        self.offline
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    fn read(&self, key: &str) -> Option<CachedResponse> {
        let raw = std::fs::read_to_string(self.path_for(key)).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// The cached body, when it was stored no longer than `max_age` ago.
    pub fn get(&self, key: &str, max_age: std::time::Duration) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let entry = self.read(key)?;
        let age = Utc::now().timestamp() - entry.fetched_at;
        // A negative age means the system clock moved back: treat it as fresh.
        (age <= max_age.as_secs() as i64).then_some(entry.body)
    }

    /// The cached body, when it is still inside the stale limit.
    pub fn get_stale(&self, key: &str, max_stale: std::time::Duration) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let entry = self.read(key)?;
        let age = Utc::now().timestamp() - entry.fetched_at;
        (age <= max_stale.as_secs() as i64).then_some(entry.body)
    }

    pub fn put(&self, key: &str, url: &str, body: &str) {
        if !self.enabled {
            return;
        }
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let entry = CachedResponse {
            url: url.to_string(),
            fetched_at: Utc::now().timestamp(),
            body: body.to_string(),
        };
        if let Ok(raw) = serde_json::to_string(&entry) {
            let _ = std::fs::write(self.path_for(key), raw);
        }
    }

    /// Number of cached responses on disk.
    pub fn len(&self) -> usize {
        if !self.enabled {
            return 0;
        }
        std::fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| entry.path().extension().is_some())
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes every cached response.
    pub fn clear(&self) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        match std::fs::remove_dir_all(&self.dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Default for ResponseCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Deterministic content key for a request URL: FNV-1a 64, so the same request
/// maps to the same filename across runs and Rust versions.
pub fn request_key(url: &str, query: &[(&str, String)]) -> String {
    let mut canonical = String::with_capacity(url.len() + 64);
    canonical.push_str(url);
    for (name, value) in query {
        canonical.push('\u{1f}');
        canonical.push_str(name);
        canonical.push('=');
        canonical.push_str(value);
    }

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in canonical.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nowcast::{NowcastPoint, NowcastProvider};
    use chrono::TimeZone;
    use std::time::Duration;

    fn sample() -> RainNowcast {
        RainNowcast::new(
            vec![NowcastPoint {
                time: Utc.with_ymd_and_hms(2026, 10, 11, 15, 0, 0).unwrap(),
                intensity_mm_per_hour: 1.5,
                interval_minutes: 5,
            }],
            NowcastProvider::Knmi,
            true,
        )
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustybui-cache-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn cycle_start_snaps_to_five_minutes() {
        let now = Utc.with_ymd_and_hms(2026, 10, 11, 15, 43, 59).unwrap();
        let start = cycle_start(now);
        assert_eq!(start % NOWCAST_CYCLE_SECONDS, 0);
        assert_eq!(start, Utc.with_ymd_and_hms(2026, 10, 11, 15, 40, 0).unwrap().timestamp());
        assert_eq!(cycle_end(start) - start, 300);
        assert_eq!(
            snap_to_cycle(now),
            Utc.with_ymd_and_hms(2026, 10, 11, 15, 40, 0).unwrap()
        );
    }

    #[test]
    fn stored_nowcast_is_returned_for_the_same_and_later_cycles() {
        let cache = DiskCache::at(temp_dir("same-cycle"));
        let location = Location::new("Leiden", 52.16, 4.49);

        assert!(cache.load_nowcast(&location, 1000).is_none());
        cache.store_nowcast(&location, 1500, &sample());
        assert!(cache.load_nowcast(&location, 1500).is_some());
        // A later cycle may reuse an older entry (clock jumps); an older cycle may not.
        assert!(cache.load_nowcast(&location, 1200).is_some());
        assert!(cache.load_nowcast(&location, 1800).is_none());

        cache.invalidate(&location);
        assert!(cache.load_nowcast(&location, 1500).is_none());
    }

    #[test]
    fn disabled_cache_never_writes() {
        let cache = DiskCache::disabled();
        let location = Location::new("Leiden", 52.16, 4.49);
        cache.store_nowcast(&location, 1500, &sample());
        assert!(cache.load_nowcast(&location, 1500).is_none());
    }

    #[test]
    fn response_cache_returns_fresh_entries_and_rejects_old_ones() {
        let cache = ResponseCache::at(temp_dir("response-fresh"));
        let key = request_key("https://example.test/wms", &[("X", "1".to_string())]);

        assert!(cache.get(&key, Duration::from_secs(300)).is_none());
        cache.put(&key, "https://example.test/wms?X=1", "<html>ok</html>");

        assert_eq!(
            cache.get(&key, Duration::from_secs(300)).as_deref(),
            Some("<html>ok</html>")
        );

        // Age the entry past the window without sleeping.
        let entry = CachedResponse {
            url: "https://example.test/wms?X=1".to_string(),
            fetched_at: Utc::now().timestamp() - 400,
            body: "<html>old</html>".to_string(),
        };
        std::fs::write(cache.path_for(&key), serde_json::to_string(&entry).unwrap()).unwrap();

        assert!(cache.get(&key, Duration::from_secs(300)).is_none());
        assert!(cache.get(&key, Duration::from_secs(600)).is_some());
        assert_eq!(cache.len(), 1);
        cache.clear().unwrap();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn response_cache_serves_stale_entries_within_the_limit() {
        let cache = ResponseCache::at(temp_dir("response-stale"));
        let key = request_key("https://example.test/radar", &[]);
        cache.put(&key, "https://example.test/radar", "{\"radar\":[]}");
        assert!(cache.get_stale(&key, Duration::from_secs(3600)).is_some());

        // Age the entry by rewriting its timestamp, then check both limits.
        let entry = CachedResponse {
            url: "https://example.test/radar".to_string(),
            fetched_at: Utc::now().timestamp() - 7200,
            body: "{\"radar\":[]}".to_string(),
        };
        std::fs::write(cache.path_for(&key), serde_json::to_string(&entry).unwrap()).unwrap();

        assert!(cache.get_stale(&key, Duration::from_secs(3600)).is_none());
        assert!(cache.get_stale(&key, Duration::from_secs(10_000)).is_some());
        assert!(ResponseCache::at(cache.dir().to_path_buf()).offline().is_offline());
    }

    #[test]
    fn request_keys_are_stable_and_distinguish_parameters() {
        let first = request_key("https://x.test/a", &[("TIME", "15:40".to_string())]);
        let second = request_key("https://x.test/a", &[("TIME", "15:40".to_string())]);
        let other = request_key("https://x.test/a", &[("TIME", "15:45".to_string())]);

        assert_eq!(first, second);
        assert_ne!(first, other);
        assert_eq!(first.len(), 16);
    }
}
