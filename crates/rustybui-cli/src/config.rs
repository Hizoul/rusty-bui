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
    /// Display unit for precipitation; `metric` (mm) unless set to `imperial`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// 24-hour clock unless set to `12h`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
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
