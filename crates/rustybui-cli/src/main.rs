//! `rustybui` — the data half of the Omarchy weather port. Prints human-readable
//! tables by default, JSON (the [`NowcastView`] contract the QML front-ends use)
//! with `--json`, and Waybar-style module JSON for the bar pill.

mod config;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, Utc};
use clap::{Args, Parser, Subcommand};
use config::Config;
use rustybui_core::{
    format_millimetres, format_pressure, format_rate, format_speed, format_temperature, AppParts,
    AppView, DayView, DiskCache, Forecast, ForecastView, Location, NowcastService, NowcastView,
    OpenMeteoClient, ResponseCache,
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
    /// One day of the forecast, opened up: what a row in the window's day list expands into.
    Day(DayArgs),

    /// Everything one screen needs, in one payload (the app's input).
    App(AppArgs),

    /// Radar imagery for the map: a PNG of DWD's composite, georeferenced by its bounding box.
    Radar(RadarArgs),
    /// Air quality (European AQI).
    Air(LocationArgs),

    /// Weather warnings in force for a location, worst first.
    Alerts(LocationArgs),
    /// Find a place by name (Open-Meteo geocoding).
    Search(SearchArgs),
    /// Store the default location used when --lat/--lon are omitted.
    SetLocation(SetLocationArgs),
    /// Read or change what the app window shows: location, clock, hours, units.
    Prefs(PrefsArgs),
    /// Saved places: the ones you switch between, so coming back does not mean searching again.
    Places(PlacesArgs),
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
    /// Dump the raw normalised model instead of the labelled view.
    #[arg(long)]
    raw: bool,
    /// Hourly rows to print (default: 12).
    #[arg(long)]
    hours: Option<usize>,
    /// Daily rows to print.
    #[arg(long, default_value_t = 3)]
    days: usize,
}

#[derive(Args)]
struct AppArgs {
    #[arg(long)]
    lat: Option<f64>,
    #[arg(long)]
    lon: Option<f64>,
    #[arg(long)]
    name: Option<String>,
    /// Hourly rows to include in the payload (default: the stored preference, else 24).
    #[arg(long)]
    hours: Option<usize>,
    /// Print a short human summary instead of the payload.
    #[arg(long)]
    text: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PrefsView {
    clock24: bool,
    hours: usize,
    offline: bool,
    units: String,
    /// Which of these the user has actually chosen, so a front-end can use its own default
    /// for the rest instead of overriding it with ours.
    stored: StoredPrefs,
    /// The stored location, so a settings screen can show which place it is editing.
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<Location>,
    /// Saved places, in the order they were saved, for a settings screen to offer as chips.
    places: Vec<Location>,
    /// How many there is room for, so the screen can say so rather than let the CLI refuse.
    place_limit: usize,
}

#[derive(serde::Serialize)]
struct StoredPrefs {
    clock: bool,
    hours: bool,
    offline: bool,
    units: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PlacesView {
    places: Vec<Location>,
    limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<Location>,
}

impl PrefsView {
    fn new(config: &Config) -> Self {
        Self {
            clock24: config.clock_is_24h(),
            hours: config.window_hours(),
            offline: config.is_offline(),
            units: if config.imperial_units() { "imperial" } else { "metric" }.to_string(),
            stored: StoredPrefs {
                clock: config.clock.is_some(),
                hours: config.hours.is_some(),
                offline: config.offline.is_some(),
                units: config.units.is_some(),
            },
            location: config.location.clone(),
            places: config.places.clone(),
            place_limit: config::MAX_PLACES,
        }
    }
}

#[derive(Args)]
struct DayArgs {
    /// The day to open, `YYYY-MM-DD`. Today when omitted.
    #[arg(long)]
    date: Option<String>,
    #[arg(long)]
    lat: Option<f64>,
    #[arg(long)]
    lon: Option<f64>,
    #[arg(long)]
    name: Option<String>,
    /// `24h` or `12h` for this call; the stored preference when omitted.
    #[arg(long)]
    clock: Option<String>,
    /// Print a short table instead of the payload.
    #[arg(long)]
    text: bool,
}

#[derive(Args)]
struct RadarArgs {
    #[arg(long)]
    lat: Option<f64>,
    #[arg(long)]
    lon: Option<f64>,
    #[arg(long)]
    name: Option<String>,
    /// Map view as `minLon,minLat,maxLon,maxLat`; defaults to a box around the location.
    #[arg(long)]
    bbox: Option<String>,
    #[arg(long, default_value_t = 1200)]
    width: u32,
    #[arg(long, default_value_t = 800)]
    height: u32,
    /// Frame to draw, RFC 3339. The newest frame when omitted.
    #[arg(long)]
    time: Option<String>,
    /// List the frames the composite has instead of fetching an image.
    #[arg(long)]
    frames: bool,
    #[arg(long)]
    json: bool,
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
struct PrefsArgs {
    /// Print the stored preferences as the JSON a front-end reads.
    #[arg(long)]
    json: bool,
    /// `24h` or `12h`.
    #[arg(long)]
    clock: Option<String>,
    /// Hourly rows the window shows, 1–24.
    #[arg(long)]
    hours: Option<usize>,
    /// The window fetches nothing and reads the last cached answers.
    #[arg(long)]
    offline: Option<bool>,
    /// `metric` (mm) or `imperial`.
    #[arg(long)]
    units: Option<String>,
}

#[derive(Args)]
struct PlacesArgs {
    /// Save the stored location as a place to come back to.
    #[arg(long)]
    add: bool,
    /// Forget a saved place.
    #[arg(long, value_name = "NAME")]
    remove: Option<String>,
    /// Make a saved place the stored location.
    #[arg(long = "use", value_name = "NAME")]
    use_place: Option<String>,
    /// Print the saved places as the JSON a front-end reads.
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
    // A filter that dies quietly when its reader goes away is what a Unix tool does; panicking
    // with a backtrace is what Rust does by default. `rustybui which | head -1` should print a
    // line, not a stack trace — and a front-end that stops reading a stream must not take the
    // binary down with it.
    #[cfg(unix)]
    unsafe {
        extern "C" {
            fn signal(signal: i32, handler: usize) -> usize;
        }
        const SIGPIPE: i32 = 13;
        const SIG_DFL: usize = 0;
        signal(SIGPIPE, SIG_DFL);
    }

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
            let view = NowcastView::new(&location, &nowcast, Utc::now(), imperial);

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
                    let view = NowcastView::new(&location, &nowcast, Utc::now(), imperial);
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
            let service = service(cli.knmi_key, cli.offline)?;
            let timezone = location.timezone.clone().unwrap_or_else(|| "auto".to_string());
            // The stored preference is the default; --hours still wins.
            let hours = args.hours.unwrap_or(12);
            let clock24 = config.clock_is_24h();
            // DWD's own model inside Germany, Open-Meteo everywhere else.
            let (forecast, provider) = service.forecast(&location, &timezone).await?;

            if args.json && !args.raw {
                // The labelled view, which is what the front-ends render.
                let view = ForecastView::new(&forecast, &location, Utc::now(), hours, imperial, clock24);
                println!("{}", serde_json::to_string_pretty(&view)?);
            } else if args.json {
                println!("{}", serde_json::to_string_pretty(&forecast)?);
            } else {
                print_forecast(&location, &forecast, hours, args.days, imperial);
                println!(
                    "  source       : {} — {}",
                    provider.display_name(),
                    provider.attribution()
                );
            }
        }

        Command::App(args) => {
            let location = resolve_location(args.lat, args.lon, args.name, &config)?;
            let service = service(cli.knmi_key, cli.offline)?;
            let now = Utc::now();
            let client = OpenMeteoClient::new(service.response_cache().clone())?;
            let timezone = location.timezone.clone().unwrap_or_else(|| "auto".to_string());
            // The window's own preferences, unless it passed --hours itself.
            let hours = args.hours.unwrap_or_else(|| config.window_hours());
            let clock24 = config.clock_is_24h();

            // All three in flight at once: separate providers, one payload, and the
            // per-provider gate still paces whatever actually leaves the machine.
            let (nowcast, forecast, air, alerts) = tokio::join!(
                service.nowcast(&location, cli.refresh),
                service.forecast(&location, &timezone),
                client.fetch_air_quality(&location),
                service.alerts(&location),
            );

            let mut errors = Vec::new();
            let forecast = match forecast {
                Ok((forecast, provider)) => Some((forecast, provider)),
                Err(error) => {
                    errors.push(format!("forecast: {error}"));
                    None
                }
            };
            let air = match air {
                Ok(air) => Some(air),
                Err(error) => {
                    errors.push(format!("air: {error}"));
                    None
                }
            };
            // Warnings are the one part a screen can do without: an error here leaves the
            // list empty rather than taking the window down with it.
            let alerts = match alerts {
                Ok(alerts) => alerts,
                Err(error) => {
                    errors.push(format!("alerts: {error}"));
                    Vec::new()
                }
            };
            if nowcast.is_none() {
                errors.push("nowcast: no radar or model coverage for this location".to_string());
            }

            let view = AppView::build(
                &location,
                now,
                AppParts {
                    nowcast: nowcast.as_ref(),
                    forecast: forecast.as_ref().map(|(forecast, _)| forecast),
                    forecast_provider: forecast.as_ref().map(|(_, provider)| *provider),
                    alerts: &alerts,
                    air: air.as_ref(),
                    errors,
                },
                hours,
                imperial,
                clock24,
            );

            if args.text {
                // Warnings first: they are the one thing here that changes what a reader does.
                for alert in &view.alerts {
                    println!(
                        "warning: {} — {} ({})",
                        alert.severity_label, alert.event, alert.window_label
                    );
                }
                print_app(&view);
            } else {
                println!("{}", serde_json::to_string(&view)?);
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

        Command::Alerts(args) => {
            let location = resolve_location(args.lat, args.lon, args.name, &config)?;
            let service = service(cli.knmi_key, cli.offline)?;
            let alerts = service.alerts(&location).await?;

            if args.json {
                println!("{}", serde_json::to_string_pretty(&alerts)?);
            } else if alerts.is_empty() {
                println!("Warnings · {}: none in force", location.name);
            } else {
                println!("Warnings · {} · {} in force", location.name, alerts.len());
                for alert in &alerts {
                    println!("  {} — {}", alert.severity.label(), alert.event);
                    if !alert.headline.is_empty() {
                        println!("      {}", alert.headline);
                    }
                    if let Some(instruction) = &alert.instruction {
                        println!("      {}", instruction);
                    }
                }
                println!("  source       : {}", alerts[0].attribution);
            }
        }

        Command::Day(args) => {
            let location = resolve_location(args.lat, args.lon, args.name, &config)?;
            let clock24 = args.clock.as_deref().map_or(config.clock_is_24h(), |clock| {
                !clock.eq_ignore_ascii_case("12h")
            });
            let date = match args.date.as_deref() {
                Some(value) => NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .with_context(|| format!("--date wants YYYY-MM-DD, not {value:?}"))?,
                None => Utc::now().date_naive(),
            };

            let service = service(cli.knmi_key, cli.offline)?;
            let timezone = location.timezone.clone().unwrap_or_else(|| "auto".to_string());
            let (forecast, provider) = service.forecast_on(&location, &timezone, date).await?;
            let view = DayView::build(&location, &forecast, date, provider, imperial, clock24);

            if args.text {
                print_day(&view);
            } else {
                println!("{}", serde_json::to_string(&view)?);
            }
        }

        Command::Radar(args) => {
            let location = resolve_location(args.lat, args.lon, args.name, &config)?;
            let service = service(cli.knmi_key, cli.offline)?;
            let bbox = match args.bbox.as_deref() {
                Some(value) => parse_bbox(value)?,
                // About 400 km each way around the place, which is a few hours of rain in any
                // direction — enough to pan around inside without asking again.
                None => [
                    location.longitude - 3.5,
                    location.latitude - 2.2,
                    location.longitude + 3.5,
                    location.latitude + 2.2,
                ],
            };

            // The frame list rides along with the image: one process spawn feeds a whole map.
            // A failure here is not fatal — the newest frame is still an image — but it is
            // said out loud rather than swallowed.
            let frames = match service.radar_frames().await {
                Ok(frames) => frames,
                Err(error) => {
                    eprintln!("radar: no frame list: {error}");
                    Vec::new()
                }
            };
            let frames_ms: Vec<i64> = frames.iter().map(|frame| frame.timestamp_millis()).collect();

            if args.frames {
                if args.json {
                    let view = serde_json::json!({
                        "framesMs": frames_ms,
                        "attribution": rustybui_core::RADAR_ATTRIBUTION,
                    });
                    println!("{}", serde_json::to_string(&view)?);
                } else if frames.is_empty() {
                    println!("Radar · no frames listed by the service");
                } else {
                    for frame in &frames {
                        println!("{}", frame.format("%Y-%m-%dT%H:%MZ"));
                    }
                }
                return Ok(());
            }

            let time = parse_instant(args.time.as_deref())?;
            let image = service.radar_image(bbox, args.width, args.height, time).await?;

            if args.json {
                let view = serde_json::json!({
                    "image": image,
                    "framesMs": frames_ms,
                    "attribution": rustybui_core::RADAR_ATTRIBUTION,
                });
                println!("{}", serde_json::to_string(&view)?);
            } else {
                let frame = DateTime::from_timestamp_millis(image.time_ms)
                    .map(|frame| frame.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "Radar · {}×{} · frame {frame} UTC{}",
                    image.width,
                    image.height,
                    if image.cached { " (cached)" } else { "" }
                );
                println!(
                    "  covers  lon {:.3}..{:.3}, lat {:.3}..{:.3}",
                    image.bbox[0], image.bbox[2], image.bbox[1], image.bbox[3]
                );
                println!("  file    {}", image.path.display());
                println!("  frames  {}", frames.len());
                println!("  {}", image.attribution);
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

        Command::Prefs(args) => {
            let mut config = config;
            // Only what was asked for changes; everything else keeps its stored value.
            let changed = args.clock.is_some()
                || args.hours.is_some()
                || args.offline.is_some()
                || args.units.is_some();

            if let Some(clock) = args.clock {
                config.clock = Some(clock);
            }
            if let Some(hours) = args.hours {
                config.hours = Some(hours.clamp(1, 24));
            }
            if let Some(offline) = args.offline {
                config.offline = Some(offline);
            }
            if let Some(units) = args.units {
                config.units = Some(units);
            }

            if changed {
                config.save()?;
            }

            if args.json {
                println!("{}", serde_json::to_string(&PrefsView::new(&config))?);
            } else {
                println!("clock   : {}", if config.clock_is_24h() { "24h" } else { "12h" });
                println!("hours   : {}", config.window_hours());
                println!("offline : {}", config.is_offline());
                println!(
                    "units   : {}",
                    if config.imperial_units() { "imperial" } else { "metric" }
                );
                match &config.location {
                    Some(location) => println!("location: {location}"),
                    None => println!("location: none stored (see `rustybui set-location`)"),
                }
                if config.places.is_empty() {
                    println!("places  : none saved (see `rustybui places --add`)");
                } else {
                    let mut names: Vec<&str> =
                        config.places.iter().map(|place| place.name.as_str()).collect();
                    names.sort_unstable_by_key(|name| name.to_lowercase());
                    println!("places  : {}", names.join(", "));
                }
            }
        }

        Command::Places(args) => {
            let mut config = config;

            if args.add {
                let place = config
                    .location
                    .clone()
                    .context("no location stored yet — search for one first, then save it")?;
                let known = config.has_place(&place.name);
                match config.save_place(place.clone()) {
                    Ok(()) => {
                        let path = config.save()?;
                        println!(
                            "{} {} ({} in all) · config: {}",
                            if known { "updated:" } else { "saved:" },
                            place,
                            config.places.len(),
                            path.display()
                        );
                    }
                    Err(reason) => bail!("{reason}"),
                }
                return Ok(());
            }

            if let Some(name) = args.remove.as_deref() {
                match config.forget_place(name) {
                    Some(gone) => {
                        let path = config.save()?;
                        println!("forgot: {} · config: {}", gone, path.display());
                    }
                    None => bail!("{} is not a saved place — `rustybui places` lists them", name.trim()),
                }
                return Ok(());
            }

            if let Some(name) = args.use_place.as_deref() {
                match config.use_place(name) {
                    Some(place) => {
                        let path = config.save()?;
                        println!("now showing: {place} · config: {}", path.display());
                    }
                    None => bail!("{} is not a saved place — `rustybui places` lists them", name.trim()),
                }
                return Ok(());
            }

            if args.json {
                let view = PlacesView {
                    places: config.places.clone(),
                    limit: config::MAX_PLACES,
                    location: config.location.clone(),
                };
                println!("{}", serde_json::to_string(&view)?);
                return Ok(());
            }

            if config.places.is_empty() {
                println!("no saved places yet — `rustybui places --add` saves the current one");
                return Ok(());
            }
            let current = config.location.as_ref().map(|location| location.name.to_lowercase());
            for place in &config.places {
                let here = current.as_deref() == Some(place.name.to_lowercase().as_str());
                println!(
                    "{} {}{}",
                    if here { "»" } else { " " },
                    place,
                    place
                        .timezone
                        .as_ref()
                        .map(|zone| format!(" · {zone}"))
                        .unwrap_or_default()
                );
            }
            println!(
                "{} of {} saved · `--use <name>`, `--remove <name>`",
                config.places.len(),
                config::MAX_PLACES
            );
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

fn parse_bbox(value: &str) -> Result<[f64; 4]> {
    let numbers: Vec<f64> = value
        .split(',')
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .collect();
    if numbers.len() != 4 {
        bail!("--bbox wants four numbers, minLon,minLat,maxLon,maxLat — got {value:?}");
    }
    let bbox = [numbers[0], numbers[1], numbers[2], numbers[3]];
    if bbox[0] >= bbox[2] || bbox[1] >= bbox[3] {
        bail!("--bbox is inside out: minLon/maxLon or minLat/maxLat are the wrong way round");
    }
    Ok(bbox)
}

fn parse_instant(value: Option<&str>) -> Result<Option<DateTime<Utc>>> {
    match value {
        None => Ok(None),
        Some(value) => Ok(Some(
            DateTime::parse_from_rfc3339(value)
                .with_context(|| format!("--time wants an RFC 3339 instant, not {value:?}"))?
                .with_timezone(&Utc),
        )),
    }
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
            // Bare coordinates inherit the stored place's country and zone only when they are
            // that place — the same name, or no name at all. Inheriting a *different* place's
            // country sent a Dutch request about Leiden to DWD, which answered 404.
            let same_place = match (&default, &name) {
                (Some(stored), Some(given)) => stored.name.eq_ignore_ascii_case(given),
                (Some(_), None) => true,
                _ => false,
            };
            let from_default = |pick: fn(&Location) -> Option<String>| {
                if same_place {
                    default.as_ref().and_then(pick)
                } else {
                    None
                }
            };

            Ok(Location {
                name: name
                    .or_else(|| default.as_ref().map(|l| l.name.clone()))
                    .unwrap_or_else(|| format!("{latitude:.4}, {longitude:.4}")),
                latitude,
                longitude,
                country_code: from_default(|l| l.country_code.clone()),
                timezone: from_default(|l| l.timezone.clone()),
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
        "  total {}   peak {}{}",
        format_millimetres(view.summary.total_mm, imperial),
        format_rate(view.summary.peak_mm_per_hour, imperial),
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
            "  {} {} {:<24} {:>9}",
            local.format("%H:%M"),
            marker,
            "█".repeat(bar_len.max((point.mm_per_hour > 0.0) as usize)),
            format_rate(point.mm_per_hour, imperial),
        );
    }
    println!();
    println!("  {}", view.provider.attribution);
}

/// Short human summary of the app payload — the JSON is the real output; this is
/// for reading in a terminal and for smoke-testing the pipeline by eye.
fn print_day(view: &DayView) {
    println!(
        "{} · {} ({})",
        view.location.name, view.day_label, view.date
    );

    if let Some(day) = &view.day {
        let sun = match (&day.sunrise_label, &day.sunset_label) {
            (Some(rise), Some(set)) => format!(" · {rise} → {set}"),
            _ => String::new(),
        };
        println!(
            "  {} / {} · {}{}{}{}",
            day.high_label.clone().unwrap_or_else(|| "-".into()),
            day.low_label.clone().unwrap_or_else(|| "-".into()),
            day.condition_label,
            day.precipitation_label
                .as_ref()
                .map(|label| format!(" · {label}"))
                .unwrap_or_default(),
            day.uv_label
                .as_ref()
                .map(|label| format!(" · UV {label}"))
                .unwrap_or_default(),
            sun
        );
    }

    if view.hours.is_empty() {
        println!("  no hourly rows for this day");
    }
    for hour in &view.hours {
        println!(
            "  {}  {}  {}  {}",
            hour.clock_label,
            hour.temperature_label.clone().unwrap_or_else(|| "-".into()),
            hour.condition_label,
            hour.probability_label.clone().unwrap_or_default()
        );
    }

    println!(
        "  {} rows · {}{}",
        view.hours.len(),
        view.timezone,
        view.forecast_attribution
            .as_ref()
            .map(|attribution| format!(" · {attribution}"))
            .unwrap_or_default()
    );
}

fn print_app(view: &AppView) {
    println!("{}", view.headline());

    if let Some(nowcast) = &view.nowcast {
        println!(
            "  nowcast: {} · {} · {} · {} samples of {} min",
            nowcast.summary.pill,
            nowcast.summary.current_label,
            nowcast.provider.attribution,
            nowcast.points.len(),
            nowcast.slot_minutes
        );
    }

    if let Some(forecast) = &view.forecast {
        if let Some(current) = &forecast.current {
            println!(
                "  current: {} {} · {} · {}{}",
                current.temperature_label.clone().unwrap_or_else(|| "-".into()),
                current.condition_label,
                current.wind_label.clone().unwrap_or_else(|| "-".into()),
                current.humidity_label.clone().unwrap_or_else(|| "-".into()),
                current
                    .pressure_label
                    .as_ref()
                    .map(|label| format!(" · {label}"))
                    .unwrap_or_default()
            );
        }
        if let Some(today) = forecast.today() {
            println!(
                "  today:   {} {} / {} · UV {} · sunrise {} sunset {}",
                today.day_label,
                today.high_label.clone().unwrap_or_else(|| "-".into()),
                today.low_label.clone().unwrap_or_else(|| "-".into()),
                today.uv_label.clone().unwrap_or_else(|| "-".into()),
                today.sunrise_label.clone().unwrap_or_else(|| "-".into()),
                today.sunset_label.clone().unwrap_or_else(|| "-".into())
            );
        }
        println!(
            "  hourly:  {} rows from {} ({}), daily {} rows",
            forecast.hourly.len(),
            forecast.hourly.first().map(|row| row.clock_label.clone()).unwrap_or_else(|| "-".into()),
            forecast.timezone,
            forecast.daily.len()
        );
    }

    if let Some(air) = &view.air {
        println!(
            "  air:     {} {}{}",
            air.band_label,
            air.european_aqi.map_or("-".to_string(), |aqi| format!("{aqi:.0}")),
            air.pm25_label
                .as_ref()
                .map(|label| format!(" · PM2.5 {label}"))
                .unwrap_or_default()
        );
    }

    if let Some(sky) = &view.sky {
        // The sun's times come from the forecast; the moon is arithmetic over the clock.
        let sun = match (&sky.sunrise_label, &sky.sunset_label) {
            (Some(rise), Some(set)) => format!(
                "{rise} to {set} · {}{}",
                sky.daylight_label.clone().unwrap_or_else(|| "-".into()),
                sky.sun_event_label
                    .as_ref()
                    .map(|event| format!(" · {event}"))
                    .unwrap_or_default()
            ),
            _ => "no times from the provider".to_string(),
        };
        println!(
            "  sky:     sun {sun}{}",
            if sky.is_daylight { "" } else { " · night" }
        );
        println!("  moon:    {} · {}", sky.moon_phase_label, sky.moon_label);
    }

    for error in &view.errors {
        println!("  ! {error}");
    }
}

fn print_forecast(
    location: &Location,
    forecast: &Forecast,
    hours: usize,
    days: usize,
    imperial: bool,
) {
    println!("Forecast · {} · {}", location.name, forecast.timezone);

    // The location's own offset, not the machine's: a forecast for Berlin reads the same
    // wherever the CLI runs.
    let offset = FixedOffset::east_opt(forecast.utc_offset_seconds as i32)
        .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());

    if let Some(current) = &forecast.current {
        println!(
            "  now: {}{} · {}% RH · {}{}{}",
            current
                .temperature_c
                .map_or("-".into(), |v| format_temperature(v, imperial)),
            current
                .apparent_temperature_c
                .map_or(String::new(), |v| format!(
                    " (feels {})",
                    format_temperature(v, imperial)
                )),
            current.relative_humidity_pct.map_or("-".into(), |v| format!("{v:.0}")),
            current
                .wind_speed_kmh
                .map_or("-".into(), |v| format_speed(v, imperial)),
            current
                .pressure_msl_hpa
                .map_or(String::new(), |v| format!(" · {}", format_pressure(v, imperial))),
            if current.is_day == Some(false) { " · night" } else { "" }
        );
    }

    println!("\n  hourly");
    for sample in forecast.hourly_from(Utc::now(), hours) {
        let local = sample.time.with_timezone(&offset);
        println!(
            "    {}  {:>7}  {:>3}%  {:>9}  {:>4}%  wind {:>9}",
            local.format("%H:%M"),
            sample
                .temperature_c
                .map_or("-".into(), |v| format_temperature(v, imperial)),
            sample
                .relative_humidity_pct
                .map_or("-".into(), |v| format!("{v:.0}")),
            sample
                .precipitation_mm
                .map_or("-".into(), |mm| format_millimetres(mm, imperial)),
            sample
                .precipitation_probability_pct
                .map_or("-".into(), |v| format!("{v:.0}")),
            sample
                .wind_speed_kmh
                .map_or("-".into(), |v| format_speed(v, imperial))
        );
    }

    println!("\n  daily");
    for day in forecast.daily.iter().take(days) {
        println!(
            "    {}  {:>7} / {:>7}  UV {:<4}  {:>9}  sunrise {}  sunset {}",
            day.date,
            day.temperature_max_c
                .map_or("-".into(), |v| format_temperature(v, imperial)),
            day.temperature_min_c
                .map_or("-".into(), |v| format_temperature(v, imperial)),
            day.uv_index_max.map_or("-".into(), |v| format!("{v:.1}")),
            // Through the shared formatter: a raw f64 prints 0.9999999999999999.
            day.precipitation_mm
                .map_or(format_millimetres(0.0, imperial), |mm| format_millimetres(mm, imperial)),
            day.sunrise
                .map(|t| t.with_timezone(&offset).format("%H:%M").to_string())
                .unwrap_or_else(|| "-".into()),
            day.sunset
                .map(|t| t.with_timezone(&offset).format("%H:%M").to_string())
                .unwrap_or_else(|| "-".into()),
        );
    }
}
