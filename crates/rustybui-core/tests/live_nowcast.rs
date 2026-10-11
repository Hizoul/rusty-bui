//! Smoke tests against the real providers.
//!
//! Ignored by default — they need network access and hit live open-data
//! endpoints. Run them with:
//!
//! ```text
//! container/run.sh cargo test -p rustybui-core -- --ignored --test-threads=1 --nocapture
//! ```

#![allow(clippy::print_stdout)]

use chrono::Utc;
use rustybui_core::{
    cycle_start, DiskCache,
    DwdRadarClient, ForecastProvider, KnmiNowcastClient, Location, NowcastProvider, NowcastService,
    NowcastView, OpenMeteoClient, ResponseCache,
};

/// A persistent cache directory shared by all live runs, so a repeat run inside
/// the same radar cycle never re-requests anything. `container/.cache` is
/// git-ignored; override with `RUSTYBUI_TEST_CACHE`.
fn live_response_cache() -> ResponseCache {
    let dir = std::env::var("RUSTYBUI_TEST_CACHE")
        .unwrap_or_else(|_| "/work/.cache/live-tests".to_string());
    ResponseCache::at(dir)
}

fn live_service() -> NowcastService {
    NowcastService::new(None)
        .expect("service")
        .with_response_cache(live_response_cache())
        .expect("response cache")
        .with_cache(DiskCache::disabled())
}

fn leiden() -> Location {
    Location::new("Leiden", 52.16, 4.49).with_country_code("NL").with_timezone("Europe/Amsterdam")
}

fn berlin() -> Location {
    Location::new("Berlin", 52.52, 13.405).with_country_code("DE")
}

fn pune() -> Location {
    Location::new("Pune", 18.52, 73.85).with_country_code("IN")
}

/// Aachen sits inside both radar products, so KNMI and DWD can be compared.
fn aachen() -> Location {
    Location::new("Aachen", 50.78, 6.08).with_country_code("DE")
}

/// Every sample must be on the provider's grid and physically plausible.
fn assert_series_sane(label: &str, nowcast: &rustybui_core::RainNowcast, slot_minutes: i64) {
    assert!(!nowcast.points.is_empty(), "{label}: empty nowcast");

    let mut previous = nowcast.points[0].time;
    for (index, point) in nowcast.points.iter().enumerate() {
        assert_eq!(
            point.interval_minutes as i64, slot_minutes,
            "{label}: sample {index} has the wrong interval"
        );
        assert!(
            (0.0..=200.0).contains(&point.intensity_mm_per_hour),
            "{label}: implausible intensity {} at sample {index}",
            point.intensity_mm_per_hour
        );
        if index > 0 {
            assert_eq!(
                (point.time - previous).num_minutes(),
                slot_minutes,
                "{label}: samples {index} and {} are not exactly {slot_minutes} min apart",
                index - 1
            );
        }
        previous = point.time;
    }

    println!(
        "{label}: {} samples from {} to {} ({} slot min), peak {:.2} mm/h, total {:.2} mm",
        nowcast.points.len(),
        nowcast.points[0].time,
        nowcast.points.last().unwrap().time,
        nowcast.slot_minutes(),
        nowcast.max_intensity(),
        nowcast.total_millimeters(),
    );
    for point in &nowcast.points {
        println!("  {}  {:>6.2} mm/h", point.time.format("%H:%M"), point.intensity_mm_per_hour);
    }
}

#[tokio::test]
#[ignore = "live KNMI request"]
async fn knmi_radar_nowcast_covers_two_hours_for_leiden() {
    let client = KnmiNowcastClient::new(None, live_response_cache()).expect("client");
    let nowcast = client.fetch(&leiden(), Utc::now()).await.expect("KNMI nowcast");

    assert_eq!(nowcast.provider, NowcastProvider::Knmi);
    assert!(nowcast.is_radar_based);
    assert_series_sane("KNMI/Leiden", &nowcast, 5);

    // KNMI publishes 24 frames per cycle: +0 … +115 min. A missing frame is a
    // logged warning rather than a failure, so allow one to be absent.
    assert!(
        nowcast.points.len() >= 24,
        "expected the 2 h window, got {} samples",
        nowcast.points.len()
    );
    let span = (nowcast.points.last().unwrap().time - nowcast.points[0].time).num_minutes();
    assert!((110..=125).contains(&span), "window should be ~2 h, got {span} min");

    let summary = nowcast.summary(Utc::now());
    println!("KNMI summary: {}", summary.pill_text(false));
    let (network, cache, stale) = client.stats().snapshot();
    println!("KNMI responses: network {network} · cache {cache} · stale {stale}");
}

#[tokio::test]
#[ignore = "live DWD/Bright Sky request"]
async fn dwd_radar_nowcast_covers_two_hours_for_berlin() {
    let client = DwdRadarClient::new(live_response_cache()).expect("client");
    let nowcast = client.fetch(&berlin(), Utc::now()).await.expect("DWD nowcast");

    assert_eq!(nowcast.provider, NowcastProvider::Dwd);
    assert!(nowcast.is_radar_based);
    assert_series_sane("DWD/Berlin", &nowcast, 5);

    // Bright Sky returns ~24 frames of the composite's ±2 h window.
    assert!(nowcast.points.len() >= 12, "unexpectedly short DWD window");
}

#[tokio::test]
#[ignore = "live Open-Meteo request"]
async fn open_meteo_minutely15_serves_locations_outside_radar_coverage() {
    let client = OpenMeteoClient::new(live_response_cache()).expect("client");
    let nowcast = client.fetch_minutely15(&pune(), Utc::now()).await.expect("minutely_15");

    assert_eq!(nowcast.provider, NowcastProvider::OpenMeteo);
    assert!(!nowcast.is_radar_based);
    assert_series_sane("Open-Meteo/Pune", &nowcast, 15);

    let span = (nowcast.points.last().unwrap().time - nowcast.points[0].time).num_minutes();
    assert!((100..=140).contains(&span), "window should be ~2 h, got {span} min");
}

#[tokio::test]
#[ignore = "live provider chain"]
async fn provider_chain_prefers_radar_inside_coverage_and_model_outside() {
    let service = live_service();

    let dutch = service.nowcast(&leiden(), true).await.expect("Dutch nowcast");
    assert_eq!(dutch.provider, NowcastProvider::Knmi, "NL should be served by KNMI radar");

    let indian = service.nowcast(&pune(), true).await.expect("Indian nowcast");
    assert_eq!(
        indian.provider,
        NowcastProvider::OpenMeteo,
        "outside radar coverage the chain must fall through to Open-Meteo"
    );
}

#[tokio::test]
#[ignore = "live KNMI + DWD cross-check"]
async fn knmi_and_dwd_radar_reasonably_agree_at_aachen() {
    let location = aachen();
    let now = Utc::now();

    let knmi = KnmiNowcastClient::new(None, live_response_cache())
        .expect("client")
        .fetch(&location, now)
        .await
        .expect("KNMI nowcast");
    let dwd = DwdRadarClient::new(live_response_cache())
        .expect("client")
        .fetch(&location, now)
        .await
        .expect("DWD nowcast");

    println!("cross-check at {}:", location.name);
    for (left, right) in knmi.points.iter().zip(dwd.points.iter()) {
        println!(
            "  {}  KNMI {:>6.2} mm/h   DWD {:>6.2} mm/h",
            left.time.format("%H:%M"),
            left.intensity_mm_per_hour,
            right.intensity_mm_per_hour
        );
    }

    // Two independent radar products, different grids, different extrapolation
    // models: they will not agree numerically. What must hold is that both cover
    // the same window and neither collapses to nothing.
    assert!(knmi.points.len() >= 24);
    assert!(dwd.points.len() >= 12);
    let overlap = knmi
        .points
        .iter()
        .filter(|left| dwd.points.iter().any(|right| right.time == left.time))
        .count();
    assert!(overlap >= 12, "products should share at least an hour of timestamps");
}

#[tokio::test]
#[ignore = "live Open-Meteo request"]
async fn forecast_and_geocoding_work() {
    let client = OpenMeteoClient::new(live_response_cache()).expect("client");

    let forecast = client.fetch_forecast(&leiden(), "Europe/Amsterdam").await.expect("forecast");
    assert!(forecast.current.is_some(), "no current block");
    assert!(forecast.hourly.len() >= 24, "hourly: {}", forecast.hourly.len());
    assert!(forecast.daily.len() >= 3, "daily: {}", forecast.daily.len());

    let current = forecast.current.as_ref().unwrap();
    let temperature = current.temperature_c.expect("temperature");
    assert!((-60.0..=60.0).contains(&temperature), "implausible temperature {temperature}");

    let today = forecast.today().expect("today");
    assert!(today.temperature_max_c.unwrap() >= today.temperature_min_c.unwrap());
    assert!(today.sunrise.is_some() && today.sunset.is_some());
    println!(
        "forecast: {:.1}°C now, {:.1}°/{:.1}° today, {} hourly, {} daily",
        temperature,
        today.temperature_max_c.unwrap(),
        today.temperature_min_c.unwrap(),
        forecast.hourly.len(),
        forecast.daily.len()
    );

    let places = client.search_places("Leiden", 5).await.expect("geocoding");
    assert!(!places.is_empty(), "geocoding returned nothing");
    let dutch = places
        .iter()
        .find(|place| place.country_code.as_deref() == Some("NL"))
        .expect("Leiden should be in the Netherlands");
    assert!((51.5..53.0).contains(&dutch.latitude), "unexpected latitude {}", dutch.latitude);
    println!("geocoding: {} @ {:.4}, {:.4}", dutch.name, dutch.latitude, dutch.longitude);

    let air = client.fetch_air_quality(&leiden()).await.expect("air quality");
    assert!(air.european_aqi.is_some(), "no European AQI");
    println!("air: European AQI {} ({})", air.european_aqi.unwrap(), air.band());
}

#[tokio::test]
#[ignore = "live provider chain"]
async fn view_contract_and_cache_round_trip() {
    let directory = std::env::temp_dir().join("rustybui-live-view");
    let _ = std::fs::remove_dir_all(&directory);

    let service = NowcastService::new(None)
        .expect("service")
        .with_response_cache(live_response_cache())
        .expect("response cache")
        .with_cache(DiskCache::at(&directory));
    let location = leiden();

    let first = service.nowcast(&location, true).await.expect("first fetch");
    let view = NowcastView::new(&location, &first, Utc::now(), false);
    let json = serde_json::to_value(&view).expect("serialise");

    assert_eq!(json["provider"]["id"], "knmi");
    assert_eq!(json["location"]["name"], "Leiden");
    assert_eq!(json["slotMinutes"], 5);
    assert_eq!(json["points"].as_array().unwrap().len(), first.points.len());
    assert_eq!(
        json["points"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|point| point["isNow"] == true)
            .count(),
        1,
        "exactly one sample should be marked as now"
    );
    println!("view: {}", view.sentence());

    // The second call must come from the cache for the same 5-minute cycle.
    let cycle = cycle_start(Utc::now());
    let cached = service
        .cache()
        .load_nowcast(&location, cycle)
        .expect("nowcast should be cached for the current cycle");
    assert_eq!(cached.points.len(), first.points.len());
    let _ = std::fs::remove_dir_all(&directory);
}

#[tokio::test]
#[ignore = "live provider chain"]
async fn a_repeat_run_inside_the_same_cycle_never_re_requests() {
    let service = live_service();
    let location = leiden();

    let cycle_before = cycle_start(Utc::now());
    let first = service.nowcast(&location, true).await.expect("first nowcast");
    let (network_first, _, _) = service.stats().0.snapshot();

    let second = service.nowcast(&location, true).await.expect("second nowcast");
    let (network_second, cached_second, _) = service.stats().0.snapshot();
    let cycle_after = cycle_start(Utc::now());

    assert_eq!(first.points.len(), second.points.len());

    if cycle_before == cycle_after {
        assert_eq!(
            network_second, network_first,
            "a second run inside the same radar cycle must not touch KNMI again"
        );
        assert!(
            cached_second >= 20,
            "expected the 24 point queries to come from cache, got {cached_second}"
        );
        println!("repeat run: network {network_second} · cache {cached_second}");
    } else {
        // The boundary rolled between the two calls; nothing to prove here.
        println!("cycle rolled during the test, skipping the cache assertion");
    }
}

#[tokio::test]
#[ignore = "live provider chain"]
async fn offline_mode_answers_from_disk_without_network() {
    // Prime the cache (a no-op when an earlier run already populated it).
    let location = leiden();
    let _ = live_service().nowcast(&location, true).await.expect("priming fetch");

    // Now go offline and force a refetch: it must be served from disk.
    let offline = NowcastService::new(None)
        .expect("service")
        .with_response_cache(live_response_cache().offline())
        .expect("response cache")
        .with_cache(DiskCache::disabled());

    let nowcast = offline.nowcast(&location, true).await.expect("offline nowcast");
    assert!(nowcast.points.len() >= 24, "offline run lost the window");

    let (knmi, dwd, open_meteo) = offline.stats();
    assert_eq!(knmi.snapshot().0, 0, "offline mode must send no KNMI request");
    assert_eq!(dwd.snapshot().0, 0, "offline mode must send no DWD request");
    assert_eq!(open_meteo.snapshot().0, 0, "offline mode must send no Open-Meteo request");
    println!(
        "offline: {} samples from {} (KNMI stale-cache serves: {})",
        nowcast.points.len(),
        nowcast.provider.display_name(),
        knmi.snapshot().2
    );
}

#[tokio::test]
#[ignore = "live provider chain"]
async fn a_german_location_reads_dwd_and_a_dutch_one_does_not() {
    let service = live_service();

    let (forecast, provider) = service
        .forecast(&berlin(), "Europe/Berlin")
        .await
        .expect("Berlin forecast");
    assert_eq!(provider, ForecastProvider::Dwd, "Berlin belongs to DWD's own model");
    assert!(forecast.current.is_some(), "DWD gave no current conditions");
    assert!(
        forecast.hourly.len() >= 24,
        "expected at least a day of hours, got {}",
        forecast.hourly.len()
    );
    assert!(
        forecast.daily.len() >= 2,
        "expected day rows to be aggregated, got {}",
        forecast.daily.len()
    );

    let current = forecast.current.as_ref().unwrap();
    let temperature = current.temperature_c.expect("temperature");
    assert!((-60.0..=60.0).contains(&temperature), "implausible temperature {temperature}");
    assert!(current.weather_code.is_some(), "the DWD icon did not become a weather code");
    assert_eq!(current.apparent_temperature_c, None, "DWD publishes no apparent temperature");
    assert_eq!(current.uv_index, None, "DWD publishes no UV index");

    let first_day = &forecast.daily[0];
    assert!(first_day.weather_code.is_some(), "the day row lost its condition");
    assert!(
        first_day.temperature_max_c.unwrap() >= first_day.temperature_min_c.unwrap(),
        "day high below its low"
    );
    assert_eq!(first_day.sunrise, None, "Bright Sky carries no sun times");
    println!(
        "Berlin: {} · {} hourly · {} daily · {} {:.1}°/{:.1}°",
        provider.display_name(),
        forecast.hourly.len(),
        forecast.daily.len(),
        first_day.date,
        first_day.temperature_max_c.unwrap(),
        first_day.temperature_min_c.unwrap()
    );

    // Same call, a Dutch location: Open-Meteo answers, because DWD's model is not the one
    // that reports on the Netherlands.
    let (_, dutch) = service
        .forecast(&leiden(), "Europe/Amsterdam")
        .await
        .expect("Leiden forecast");
    assert_eq!(dutch, ForecastProvider::OpenMeteo, "Leiden belongs to Open-Meteo");

    // Aachen is a German location just over the border, and Open-Meteo could serve it too;
    // DWD is still the one that reports on it.
    let (_, aachen_provider) = service
        .forecast(&aachen(), "Europe/Berlin")
        .await
        .expect("Aachen forecast");
    assert_eq!(aachen_provider, ForecastProvider::Dwd);
    println!("Aachen: {}", aachen_provider.display_name());
}

#[tokio::test]
#[ignore = "live provider chain"]
async fn warnings_come_from_dwd_and_nowhere_else() {
    let service = live_service();

    // Berlin is inside DWD's warn cells: a real answer, which on a quiet day is an empty list.
    let german = service.alerts(&berlin()).await.expect("Berlin warnings");
    for alert in &german {
        assert!(!alert.event.is_empty(), "a warning with no event");
        assert!(!alert.id.is_empty(), "a CAP warning with no identifier");
        assert!(!alert.attribution.is_empty(), "a warning with no attribution");
        assert!(alert.onset.is_some() || alert.expires.is_some(), "a warning with no window");
    }
    // Worst first, so a screen showing one shows the one that matters.
    for pair in german.windows(2) {
        assert!(pair[0].severity.rank() >= pair[1].severity.rank(), "warnings out of order");
    }
    println!("Berlin: {} warning(s) in force", german.len());

    // Leiden is outside them, and KNMI's warnings need a key, so nothing is asked and
    // nothing is reported: an empty list, not an error.
    let dutch = service.alerts(&leiden()).await.expect("Leiden warnings");
    assert!(dutch.is_empty(), "expected no warnings for a location DWD does not cover");
}
