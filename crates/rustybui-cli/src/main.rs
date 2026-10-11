//! `rustybui` — the data half of the Omarchy weather port. Prints human-readable
//! tables by default, JSON (the [`NowcastView`] contract the QML front-ends use)
//! with `--json`, and Waybar-style module JSON for the bar pill.

mod config;

use anyhow::{bail, Context, Result};
use chrono::{Local, Utc};
use clap::{Args, Parser, Subcommand};
use config::Config;
use rustybui_core::{
    format_intensity, DiskCache, Forecast, Location, NowcastService, NowcastView, OpenMeteoClient,
    ResponseCache,
};

#[derive(Parser)]
#[command(
    name = "rustybui",
    version,
    about = "Rain nowcast and weather for the Omarchy bar (KNMI, DWD, Open-Meteo)"
)]
struct Cli {
    /// KNMI API key for the keyed data platform host. Anonymous access works.
    #[arg(long, env = "KNMI_API_KEY", global = true)]
    knmi_key: Option<String>,

    /// Ignore the per-radar-cycle nowcast cache (raw responses are still reused
    /// inside their own freshness window).
    #[arg(long, global = true)]
    refresh: bool,

    /// Never touch the network: answer from the on-disk response cache only.
    #[arg(long, global = true)]
    offline: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Two-hour rain nowcast (KNMI radar, DWD radar, Open-Meteo model).
    Nowcast(LocationArgs),
    /// Output for the Omarchy bar: Waybar-style JSON, or the full view with --json.
    Bar(BarArgs),
    /// Weather forecast (DWD/Bright Sky or Open-Meteo).
    Forecast(ForecastArgs),
    /// Air quality (European AQI).
    Air(LocationArgs),
    /// Find a place by name (Open-Meteo geocoding).
    Search(SearchArgs),
    /// Store the default location used when --lat/--lon are omitted.
    SetLocation(SetLocationArgs),
    /// Print the default location and config path.
    Which,
    /// Report or clear the caches.
    Cache(CacheArgs),
}

#[derive(Args)]
struct CacheArgs {
    /// Delete every cached nowcast and provider response.
    #[arg(long)]
    clear: bool,
}

#[derive(Args)]
struct LocationArgs {
    /// Latitude; defaults to the stored default location.
    #[arg(long)]
    lat: Option<f64>,
    /// Longitude; defaults to the stored default location.
    #[arg(long)]
    lon: Option<f64>,
    /// Display name; defaults to the stored one.
    #[arg(long)]
    name: Option<String>,
    /// Emit JSON instead of a table.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct BarArgs {
    #[arg(long)]
    lat: Option<f64>,
    #[arg(long)]
    lon: Option<f64>,
    #[arg(long)]
    name: Option<String>,
    /// Print the whole nowcast view (what the popout and the app consume).
    #[arg(long)]
    json: bool,
    /// Print bare text instead of Waybar JSON.
    #[arg(long)]
    text: bool,
}

#[derive(Args)]
struct ForecastArgs {
    #[arg(long)]
    lat: Option<f64>,
    #[arg(long)]
    lon: Option<f64>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    json: bool,
    /// Hourly rows to print.
    #[arg(long, default_value_t = 12)]
    hours: usize,
    /// Daily rows to print.
    #[arg(long, default_value_t = 3)]
    days: usize,
}

#[derive(Args)]
struct SearchArgs {
    /// Place name, e.g. "Leiden".
    query: String,
    /// Maximum results.
    #[arg(long, default_value_t = 5)]
    count: usize,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct SetLocationArgs {
    /// Display name for the location.
    name: String,
    #[arg(long)]
    lat: f64,
    #[arg(long)]
    lon: f64,
    /// ISO-3166 alpha-2 country code (decides KNMI radar coverage).
    #[arg(long)]
    country: Option<String>,
    /// IANA timezone, e.g. Europe/Amsterdam.
    #[arg(long)]
    timezone: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load();
    let imperial = config.imperial_units();

    match cli.command {
        Command::Nowcast(args) => {
            let location = resolve_location(args.lat, args.lon, args.name, &config)?;
            let service = service(cli.knmi_key, cli.offline)?;
            let nowcast = service.nowcast(&location, cli.refresh).await.with_context(|| {
                format!(
                    "no rain nowcast for {location}: KNMI and DWD radar do not cover it and \
Open-Meteo returned nothing"
                )
            })?;
            let view = NowcastView::new(&location, &nowcast, Utc::now());

            if args.json {
                println!("{}", serde_json::to_string_pretty(&view)?);
            } else {
                print_nowcast(&view, imperial);
                print_stats(&service);
            }
        }

        Command::Bar(args) => {
            let location = resolve_location(args.lat, args.lon, args.name, &config)?;
            let service = service(cli.knmi_key, cli.offline)?;

            match service.nowcast(&location, cli.refresh).await {
                Some(nowcast) => {
                    let view = NowcastView::new(&location, &nowcast, Utc::now());
                    if args.json {
                        println!("{}", serde_json::to_string(&view)?);
                    } else if args.text {
                        println!("{}", view.bar_text());
                    } else {
                        // Waybar/Omarchy inline module contract.
                        let payload = serde_json::json!({
                            "text": view.bar_text(),
                            "alt": view.bar_class(),
                            "class": view.bar_class(),
                            "tooltip": view.sentence(),
                        });
                        println!("{payload}");
                    }
                }
                None if args.json => {
                    // The popout treats a null payload as "no data for this place".
                    println!("null");
                }
                None => {
                    println!(
                        "{}",
                        serde_json::json!({ "text": "--", "class": "unknown", "tooltip": "no nowcast available" })
                    );
                }
            }
        }

        Command::Forecast(args) => {
            let location = resolve_location(args.lat, args.lon, args.name, &config)?;
            let client = OpenMeteoClient::new(ResponseCache::new())?;
            let timezone = location.timezone.clone().unwrap_or_else(|| "auto".to_string());
            let forecast = client.fetch_forecast(&location, &timezone).await?;

            if args.json {
                println!("{}", serde_json::to_string_pretty(&forecast)?);
            } else {
                print_forecast(&location, &forecast, args.hours, args.days, imperial);
            }
        }

        Command::Air(args) => {
            let location = resolve_location(args.lat, args.lon, args.name, &config)?;
            let client = OpenMeteoClient::new(ResponseCache::new())?;
            let air = client.fetch_air_quality(&location).await?;

            if args.json {
                println!("{}", serde_json::to_string_pretty(&air)?);
            } else {
                println!("Air quality · {}", location.name);
                println!(
                    "  European AQI : {} ({})",
                    air.european_aqi.map_or("-".to_string(), |v| format!("{v:.0}")),
                    air.band()
                );
                println!(
                    "  US AQI       : {}",
                    air.us_aqi.map_or("-".to_string(), |v| format!("{v:.0}"))
                );
                println!(
                    "  PM2.5 / PM10 : {} / {} µg/m³",
                    air.pm2_5_ug_m3.map_or("-".to_string(), |v| format!("{v:.1}")),
                    air.pm10_ug_m3.map_or("-".to_string(), |v| format!("{v:.1}"))
                );
            }
        }

        Command::Search(args) => {
            let client = OpenMeteoClient::new(ResponseCache::new())?;
            let places = client.search_places(&args.query, args.count).await?;

            if args.json {
                println!("{}", serde_json::to_string_pretty(&places)?);
            } else if places.is_empty() {
                println!("no matches for {:?}", args.query);
            } else {
                for place in &places {
                    let country = place.country_code.clone().unwrap_or_default();
                    println!(
                        "{:<38} {:>9.4}, {:>9.4}  {}",
                        place.name, place.latitude, place.longitude, country
                    );
                }
            }
        }

        Command::SetLocation(args) => {
            let mut location = Location::new(args.name, args.lat, args.lon);
            location.country_code = args.country.map(|code| code.to_uppercase());
            location.timezone = args.timezone;
            let mut config = config;
            config.location = Some(location.clone());
            let path = config.save()?;
            println!("default location: {location}\nconfig: {}", path.display());
        }

        Command::Which => {
            match &config.location {
                Some(location) => println!("{location}"),
                None => println!("no default location stored yet (see `rustybui set-location`)"),
            }
            if let Some(path) = Config::path() {
                println!("config:  {}", path.display());
            }
            let nowcasts = DiskCache::new();
            let responses = ResponseCache::new();
            println!("nowcasts: {}", nowcasts.dir().display());
            println!("responses: {}", responses.dir().display());
        }

        Command::Cache(args) => {
            let nowcasts = DiskCache::new();
            let responses = ResponseCache::new();

            if args.clear {
                nowcasts.clear().with_context(|| {
                    format!("clearing {}", nowcasts.dir().display())
                })?;
                responses.clear().with_context(|| {
                    format!("clearing {}", responses.dir().display())
                })?;
                println!("caches cleared");
            } else {
                println!("nowcasts:   {}", nowcasts.dir().display());
                println!("responses:  {}", responses.dir().display());
                println!("cached responses: {}", responses.len());
            }
        }
    }

    Ok(())
}

/// One service for every command: shared response cache, politeness gate, and
/// the optional offline switch.
fn service(knmi_key: Option<String>, offline: bool) -> Result<NowcastService> {
    Ok(NowcastService::new(knmi_key)?
        .offline(offline)?
        .with_cache(DiskCache::new()))
}

/// Where the answers came from, so it is obvious when a run was served from disk.
fn print_stats(service: &NowcastService) {
    let (knmi, dwd, open_meteo) = service.stats();
    let line = |name: &str, (network, cache, stale): (u64, u64, u64)| {
        if network + cache + stale == 0 {
            format!("  {name:<11} no requests")
        } else {
            format!("  {name:<11} network {network} · cache {cache} · stale {stale}")
        }
    };
    println!();
    println!("responses");
    println!("{}", line("KNMI", knmi.snapshot()));
    println!("{}", line("DWD", dwd.snapshot()));
    println!("{}", line("Open-Meteo", open_meteo.snapshot()));
}

fn resolve_location(
    lat: Option<f64>,
    lon: Option<f64>,
    name: Option<String>,
    config: &Config,
) -> Result<Location> {
    match (lat, lon) {
        (Some(latitude), Some(longitude)) => {
            let default = config.location.clone();
            Ok(Location {
                name: name
                    .or_else(|| default.as_ref().map(|l| l.name.clone()))
                    .unwrap_or_else(|| format!("{latitude:.4}, {longitude:.4}")),
                latitude,
                longitude,
                country_code: default.and_then(|l| l.country_code),
                timezone: None,
            })
        }
        (None, None) => match (config.location.clone(), name) {
            (Some(mut location), Some(name)) => {
                location.name = name;
                Ok(location)
            }
            (Some(location), None) => Ok(location),
            (None, _) => bail!(
                "no location: pass --lat/--lon, or store a default with \
`rustybui set-location \"Leiden\" --lat 52.16 --lon 4.49 --country NL`"
            ),
        },
        _ => bail!("--lat and --lon must be given together"),
    }
}

fn print_nowcast(view: &NowcastView, imperial: bool) {
    let unit = if imperial { "in/h" } else { "mm/h" };
    println!(
        "Rain nowcast · {} · {}",
        view.location.name, view.provider.label
    );
    println!(
        "  now: {}   {}   window: +0…+{} min in {}-min steps",
        view.summary.pill,
        view.summary.current_label,
        view.points.len().saturating_sub(1) as u32 * view.slot_minutes,
        view.slot_minutes
    );
    if let Some(starts) = &view.summary.starts_in_label {
        println!("  starts in {}", starts);
    }
    if let Some(ends) = &view.summary.ends_in_label {
        println!("  stops in {}", ends);
    }
    println!(
        "  total {:.2} mm   peak {:.2} mm/h{}",
        view.summary.total_mm,
        view.summary.peak_mm_per_hour,
        view.summary
            .peak_in_label
            .as_ref()
            .map(|label| format!(" in {}", label.trim_start_matches("in ")))
            .unwrap_or_default()
    );
    println!();

    let max = view.summary.peak_mm_per_hour.max(0.5);
    for point in &view.points {
        let local = point.time.with_timezone(&Local);
        let bar_len = ((point.mm_per_hour / max) * 24.0).round() as usize;
        let marker = if point.is_now { "<" } else { " " };
        println!(
            "  {} {} {:<24} {:>7} {}",
            local.format("%H:%M"),
            marker,
            "█".repeat(bar_len.max((point.mm_per_hour > 0.0) as usize)),
            format_intensity(point.mm_per_hour),
            unit
        );
    }
    println!();
    println!("  {}", view.provider.attribution);
}

fn print_forecast(
    location: &Location,
    forecast: &Forecast,
    hours: usize,
    days: usize,
    imperial: bool,
) {
    println!("Forecast · {} · {}", location.name, forecast.timezone);
    if let Some(current) = &forecast.current {
        println!(
            "  now: {}°C (feels {}°C) · {}% RH · {} km/h · {} hPa{}",
            current.temperature_c.map_or("-".into(), |v| format!("{v:.1}")),
            current
                .apparent_temperature_c
                .map_or("-".into(), |v| format!("{v:.1}")),
            current.relative_humidity_pct.map_or("-".into(), |v| format!("{v:.0}")),
            current.wind_speed_kmh.map_or("-".into(), |v| format!("{v:.1}")),
            current.pressure_msl_hpa.map_or("-".into(), |v| format!("{v:.0}")),
            if current.is_day == Some(false) { " · night" } else { "" }
        );
    }

    println!("\n  hourly");
    for sample in forecast.hourly_from(Utc::now(), hours) {
        let local = sample.time.with_timezone(&Local);
        println!(
            "    {}  {:>5}°C  {:>3}%  {:>5} {}  {:>4}%  wind {:>5} km/h",
            local.format("%H:%M"),
            sample.temperature_c.map_or("-".into(), |v| format!("{v:.1}")),
            sample
                .relative_humidity_pct
                .map_or("-".into(), |v| format!("{v:.0}")),
            sample
                .precipitation_mm
                .map(|mm| if imperial {
                    format!("{:.3}", mm / 25.4)
                } else {
                    format!("{mm:.2}")
                })
                .unwrap_or_else(|| "-".into()),
            if imperial { "in" } else { "mm" },
            sample
                .precipitation_probability_pct
                .map_or("-".into(), |v| format!("{v:.0}")),
            sample.wind_speed_kmh.map_or("-".into(), |v| format!("{v:.1}"))
        );
    }

    println!("\n  daily");
    for day in forecast.daily.iter().take(days) {
        println!(
            "    {}  {:>5}° / {:>5}°C  UV {:<4}  {:>5} mm  sunrise {}  sunset {}",
            day.date,
            day.temperature_max_c.map_or("-".into(), |v| format!("{v:.1}")),
            day.temperature_min_c.map_or("-".into(), |v| format!("{v:.1}")),
            day.uv_index_max.map_or("-".into(), |v| format!("{v:.1}")),
            day.precipitation_mm.map_or(0.0, |v| v),
            day.sunrise
                .map(|t| t.with_timezone(&Local).format("%H:%M").to_string())
                .unwrap_or_else(|| "-".into()),
            day.sunset
                .map(|t| t.with_timezone(&Local).format("%H:%M").to_string())
                .unwrap_or_else(|| "-".into()),
        );
    }
}
