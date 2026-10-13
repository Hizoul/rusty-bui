//! Config file: the default location the bar pill and the app use.
//!
//! `~/.config/rustybui/config.json`, overridable with `RUSTYBUI_CONFIG`.

use anyhow::{Context, Result};
use rustybui_core::Location;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}

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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let body = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    pub fn imperial_units(&self) -> bool {
        self.units.as_deref().is_some_and(|units| units.eq_ignore_ascii_case("imperial"))
    }
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

    #[test]
    fn a_config_written_before_there_were_places_still_reads() {
        let old = r#"{"location":{"name":"Leiden","latitude":52.16,"longitude":4.49},"units":"imperial"}"#;
        let config: Config = serde_json::from_str(old).expect("an older config is still a config");
        assert!(config.places.is_empty());
        assert!(config.imperial_units());
        assert_eq!(config.location.map(|location| location.name), Some("Leiden".to_string()));
    }
}
