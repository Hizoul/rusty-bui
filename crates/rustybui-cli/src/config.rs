//! Config file: the default location the bar pill and the app use.
//!
//! `~/.config/rustybui/config.json`, overridable with `RUSTYBUI_CONFIG`.

use anyhow::{Context, Result};
use rustybui_core::{DurationStyle, Location};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// Places worth switching back to, in the order they were saved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub places: Vec<Location>,
    /// Display unit for precipitation; `metric` (mm) unless set to `imperial`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// 24-hour clock unless set to `12h`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
    /// Hourly rows the app window asks for, 1–24. Absent means 24.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours: Option<usize>,
    /// The app window reads the last cached answers and fetches nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline: Option<bool>,
    /// How spans of minutes are written: `colon` (1:31h), `words` (1 h 31 min) or `compact`
    /// (1h31). Absent means colon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    /// The window's cards, top to bottom. Absent means [`DEFAULT_CARDS`]; unknown ids are
    /// dropped and missing ones appended on the way out, so a stale file cannot lose a card.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub card_order: Vec<String>,
    /// Cards the user has switched off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_cards: Vec<String>,
}

/// The cards a person can move and hide, in the order the window ships them. The settings,
/// warnings and footer are page furniture, and an opened day stays glued to the days card.
pub const DEFAULT_CARDS: [&str; 7] = ["hours", "air", "sky", "days", "now", "nowcast", "map"];

/// How many places the settings card can show in one go, and so how many are kept. A cap
/// rather than a scroll: the row is for the four or five places a person actually lives in.
pub const MAX_PLACES: usize = 12;

/// Preferences a screen can change, with the defaults the CLI and the window share.
impl Config {
    /// Hourly rows the window shows: 1–24, clamped rather than rejected, because the value
    /// arrives from a UI that may hand over anything.
    pub fn window_hours(&self) -> usize {
        self.hours.unwrap_or(24).clamp(1, 24)
    }

    pub fn clock_is_24h(&self) -> bool {
        !self.clock.as_deref().is_some_and(|clock| clock.eq_ignore_ascii_case("12h"))
    }

    pub fn duration_style(&self) -> DurationStyle {
        self.duration.as_deref().and_then(DurationStyle::parse).unwrap_or_default()
    }

    /// Every card exactly once: the stored order for the ones it names, the rest after them.
    pub fn cards_in_order(&self) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        for id in self.card_order.iter().map(String::as_str).chain(DEFAULT_CARDS) {
            if DEFAULT_CARDS.contains(&id) && !order.iter().any(|known| known == id) {
                order.push(id.to_string());
            }
        }
        order
    }

    pub fn hidden_card_ids(&self) -> Vec<String> {
        self.cards_in_order().into_iter().filter(|id| self.hidden_cards.contains(id)).collect()
    }

    pub fn is_offline(&self) -> bool {
        self.offline.unwrap_or(false)
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        if let Some(path) = std::env::var_os("RUSTYBUI_CONFIG") {
            return Some(PathBuf::from(path));
        }
        if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
            if !dir.is_empty() {
                return Some(PathBuf::from(dir).join("rustybui").join("config.json"));
            }
        }
        let home = std::env::var_os("HOME")?;
        (!home.is_empty()).then(|| {
            PathBuf::from(home).join(".config").join("rustybui").join("config.json")
        })
    }

    /// Missing or unreadable config is an empty config, never an error: the CLI
    /// still works with explicit `--lat/--lon`.
    pub fn load() -> Self {
        let Some(path) = Self::path() else { return Self::default() };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::path().context("no HOME or XDG_CONFIG_HOME to write a config to")?;
        let body = serde_json::to_string_pretty(self)?;
        write_private(&path, body.as_bytes())?;
        Ok(path)
    }

    pub fn imperial_units(&self) -> bool {
        self.units.as_deref().is_some_and(|units| units.eq_ignore_ascii_case("imperial"))
    }
}

/// The config holds precise coordinates and saved places, so it is written owner-only.
fn write_private(path: &Path, body: &[u8]) -> Result<()> {
    rustybui_core::write_private(path, body).with_context(|| format!("writing {}", path.display()))
}

/// Saved places: the ones a person switches between, so that coming back does not mean
/// searching again.
///
/// Matched by name, case-insensitively, because the name is what the user sees and clicks —
/// the same place arriving twice ("Leiden", "leiden") would be a bug in the UI, not a feature.
impl Config {
    fn find_place(&self, name: &str) -> Option<usize> {
        let wanted = name.trim();
        self.places
            .iter()
            .position(|place| place.name.trim().eq_ignore_ascii_case(wanted))
    }

    pub fn has_place(&self, name: &str) -> bool {
        self.find_place(name).is_some()
    }

    /// Saves `place`, replacing an entry of the same name. Refuses once [`MAX_PLACES`] are
    /// held, naming the limit — silently dropping someone's saved place would be worse.
    pub fn save_place(&mut self, place: Location) -> Result<(), String> {
        match self.find_place(&place.name) {
            Some(at) => self.places[at] = place,
            None if self.places.len() >= MAX_PLACES => {
                return Err(format!(
                    "{MAX_PLACES} saved places is the limit — forget one first (`rustybui places --remove <name>`)"
                ))
            }
            None => self.places.push(place),
        }
        Ok(())
    }

    /// Forgets a place, returning it so the caller can say which one went.
    pub fn forget_place(&mut self, name: &str) -> Option<Location> {
        let at = self.find_place(name)?;
        Some(self.places.remove(at))
    }

    /// Makes a saved place the current one, coordinates, country and timezone and all.
    pub fn use_place(&mut self, name: &str) -> Option<Location> {
        let at = self.find_place(name)?;
        let place = self.places[at].clone();
        self.location = Some(place.clone());
        Some(place)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_order_keeps_what_it_knows_and_never_loses_a_card() {
        let mut config = Config::default();
        assert_eq!(config.cards_in_order(), DEFAULT_CARDS);
        config.card_order = vec!["air".into(), "bogus".into(), "map".into(), "air".into()];
        let order = config.cards_in_order();
        assert_eq!(&order[..2], ["air", "map"]);
        assert_eq!(order.len(), DEFAULT_CARDS.len());
        config.hidden_cards = vec!["map".into(), "bogus".into()];
        assert_eq!(config.hidden_card_ids(), ["map"]);
    }

    #[test]
    fn an_unknown_duration_falls_back_to_colon() {
        let mut config = Config::default();
        config.duration = Some("nonsense".into());
        assert_eq!(config.duration_style(), DurationStyle::Colon);
        config.duration = Some("Words".into());
        assert_eq!(config.duration_style(), DurationStyle::Words);
    }

    fn place(name: &str, latitude: f64, longitude: f64) -> Location {
        let mut location = Location::new(name, latitude, longitude);
        location.country_code = Some("NL".to_string());
        location.timezone = Some("Europe/Amsterdam".to_string());
        location
    }

    #[test]
    fn a_place_is_saved_once_and_keeps_its_order() {
        let mut config = Config::default();
        config.save_place(place("Leiden", 52.16, 4.49)).unwrap();
        config.save_place(place("Berlin", 52.52, 13.405)).unwrap();
        assert_eq!(config.places.len(), 2);
        assert_eq!(config.places[0].name, "Leiden");
        assert!(config.has_place("berlin"));
        assert!(!config.has_place("Utrecht"));
    }

    #[test]
    fn saving_the_same_place_again_replaces_it_rather_than_doubling_it() {
        // The same place coming back with better coordinates, or simply a different case:
        // one entry, and it does not move to the end of the list.
        let mut config = Config::default();
        config.save_place(place("Leiden", 52.16, 4.49)).unwrap();
        config.save_place(place("Berlin", 52.52, 13.405)).unwrap();
        config.save_place(place("leiden", 52.161, 4.491)).unwrap();

        assert_eq!(config.places.len(), 2);
        assert_eq!(config.places[0].name, "leiden");
        assert_eq!(config.places[0].latitude, 52.161);
        assert!(config.has_place("LEIDEN"));
    }

    #[test]
    fn the_limit_is_refused_by_name_not_enforced_by_dropping_someone() {
        let mut config = Config::default();
        for index in 0..MAX_PLACES {
            config.save_place(place(&format!("Place {index}"), 50.0, 5.0)).unwrap();
        }
        let refused = config.save_place(place("One Too Many", 50.0, 5.0));
        assert!(refused.is_err(), "the {MAX_PLACES}th place should have been refused");
        assert_eq!(config.places.len(), MAX_PLACES);
        assert!(!config.has_place("One Too Many"));

        // …but an existing one can still be updated, and a forgotten one makes room.
        assert!(config.save_place(place("Place 3", 51.0, 6.0)).is_ok());
        config.forget_place("Place 0").unwrap();
        assert!(config.save_place(place("One Too Many", 50.0, 5.0)).is_ok());
        assert_eq!(config.places.len(), MAX_PLACES);
    }

    #[test]
    fn forgetting_returns_what_went_and_shrugs_at_what_was_never_there() {
        let mut config = Config::default();
        config.save_place(place("Leiden", 52.16, 4.49)).unwrap();
        let gone = config.forget_place("  leiden  ").expect("found, despite the spaces");
        assert_eq!(gone.name, "Leiden");
        assert!(config.places.is_empty());
        assert!(config.forget_place("Leiden").is_none());
    }

    #[test]
    fn using_a_saved_place_makes_it_the_current_one() {
        let mut config = Config::default();
        config.save_place(place("Leiden", 52.16, 4.49)).unwrap();
        config.save_place(place("Berlin", 52.52, 13.405)).unwrap();

        let used = config.use_place("BERLIN").expect("saved");
        assert_eq!(used.name, "Berlin");
        assert!((used.longitude - 13.405).abs() < 1e-9);
        assert_eq!(config.location.as_ref().map(|l| l.name.as_str()), Some("Berlin"));
        // The country and zone come with it: switching to a German place must send DWD
        // queries, and a place stored with the wrong country would not.
        assert_eq!(config.location.as_ref().and_then(|l| l.country_code.clone()), Some("NL".to_string()));

        assert!(config.use_place("Utrecht").is_none());
        assert_eq!(config.location.as_ref().map(|l| l.name.as_str()), Some("Berlin"));
    }

    #[cfg(unix)]
    #[test]
    fn saved_config_is_owner_only_even_over_an_old_world_readable_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rustybui-perms-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("rustybui").join("config.json");

        // A new directory and file are private from the start.
        write_private(&path, b"{}").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path), 0o600);

        // An existing world-readable file (from an older version) is tightened on save.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, b"{\"units\":\"imperial\"}").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"units\":\"imperial\"}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_config_written_before_there_were_places_still_reads() {
        let old = r#"{"location":{"name":"Leiden","latitude":52.16,"longitude":4.49},"units":"imperial"}"#;
        let config: Config = serde_json::from_str(old).expect("an older config is still a config");
        assert!(config.places.is_empty());
        assert!(config.imperial_units());
        assert_eq!(config.location.map(|location| location.name), Some("Leiden".to_string()));
    }
}
