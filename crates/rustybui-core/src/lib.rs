//! Core data layer for rusty-bui: a Rust port of WeatherMaster's weather
//! pipeline for KNMI, DWD (Bright Sky) and Open-Meteo. No UI, no CLI.

pub mod alerts;
pub mod cache;
pub mod forecast;
pub mod location;
pub mod moon;
pub mod nowcast;
pub mod private_file;
pub mod providers;
pub mod radar;
pub mod solar;
pub mod units;
pub mod view;

pub mod appview;

pub use appview::{
    condition_label, wind_compass, AirQualityView, AlertView, AppParts, AppView, CurrentView,
    DailyView, DayView, ForecastView, HourlyView,
};

pub use alerts::{Alert, AlertSeverity};

pub use cache::{
    cycle_start, request_key, snap_to_cycle, DiskCache, ResponseCache, NOWCAST_CYCLE_SECONDS,
    RESPONSE_FRESH_SECONDS, RESPONSE_MAX_STALE_SECONDS,
};
pub use forecast::{CurrentWeather, DailyForecast, Forecast, HourlyForecast};
pub use location::Location;
pub use private_file::write_private;
pub use nowcast::{
    format_intensity, format_millimeters, format_minutes, intensity_class, DurationStyle, IntensityClass,
    NowcastPoint, NowcastProvider, NowcastSummary, RainNowcast, RAIN_THRESHOLD_MM_H,
};
pub use radar::{RadarImage, RadarMapClient, RADAR_ATTRIBUTION};
pub use units::{
    format_millimetres, format_pressure, format_rate, format_speed, format_temperature,
};
pub use providers::{
    BrightSkyClient, DwdRadarClient, FetchStats, ForecastProvider, HttpClient, KnmiNowcastClient,
    NowcastService, OpenMeteoClient, ProviderError, Source,
};
pub use view::NowcastView;

/// Sent on every request; the open-data endpoints are anonymous but polite
/// clients identify themselves.
pub const USER_AGENT: &str = concat!(
    "rusty-bui/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/PranshulGG/WeatherMaster; Rust port)"
);
