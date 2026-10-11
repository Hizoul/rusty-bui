# rusty-bui

Rust port of [WeatherMaster](https://github.com/PranshulGG/WeatherMaster) for the
[Omarchy](https://omarchy.org/) desktop (Hyprland + Quickshell).

WeatherMaster is an Android/Compose application (GPL-3.0). This project ports its
data pipeline to Rust and replaces the Android UI with:

1. `rustybui` — a CLI that fetches, normalises and caches weather data and emits JSON.
2. `omarchy/` — an Omarchy 4 ("Quattro") shell plugin: a bar pill showing the rain
   nowcast for the default location, with a detail popout that opens on click or on a
   Hyprland keybinding (`SUPER + SHIFT + R`).
3. `app/` — a standalone Quickshell window app (opened from the Omarchy menu) that
   renders the full weather app.

Only three providers are supported, as ported: **KNMI** (radar nowcast, NL),
**DWD** (Bright Sky weather + radar nowcast, DE) and **Open-Meteo** (weather,
air quality, geocoding, minutely-15 nowcast fallback, worldwide).

## Layout

```
crates/rustybui-core   domain types, providers, caches, the view contracts QML renders
crates/rustybui-cli    `rustybui` binary: nowcast / forecast / app / air / search / bar / cache
qml/                   QML shared by both plugins: Model.js, NowcastChart.qml, Card.qml
omarchy/               bar plugin: Pill.qml (the pill) + RainPanel.qml (the popout)
app/                   window plugin: App.qml
test/                  node checks of Model.js against captured payloads
scripts/               staging, used by install.sh and by the QML check
container/             Containerfile + helper scripts for podman
install.sh             build, install both plugins, launcher entry, keybindings
```

The two plugin directories link to the shared files in `qml/` so an editor and `qmllint`
resolve them in place; staging dereferences the links, because Omarchy rejects symlinks
inside a plugin folder.

## Building and testing (podman)

The host has no toolchain; everything runs in a container matching Omarchy's Arch base.

```bash
container/build.sh                             # build the image
container/run.sh cargo test                    # unit tests (offline fixtures)
container/run.sh cargo test -p rustybui-core --test live_nowcast -- --ignored --test-threads=1
container/run.sh cargo run -p rustybui-cli -- nowcast --lat 52.16 --lon 4.49
```

Live tests hit the real endpoints, so they share one persistent cache directory
(`.cache/live-tests`, git-ignored): a repeat run inside the same radar cycle sends no
requests at all.

The QML is checked without a running shell:

```bash
node test/model.test.mjs                          # payload and label logic
container/check-qml.sh /path/to/omarchy-checkout  # manifests + QML against the real shell
```

## The Omarchy plugins

Two plugins, both thin: they own no fetching, they run `rustybui` and render the result,
so all caching and rate limiting stays in one place.

- `omarchy/` — the bar pill and its popout: the two-hour nowcast for the stored location,
  in the bar and in a popout on `SUPER + SHIFT + R`.
- `app/` — the weather window: current conditions, the same nowcast chart, hourly and
  daily outlook, air quality, opened from the launcher (installed as a `.desktop` entry)
  or with `omarchy-shell shell toggle mmb.weather '{}'`.

On the Omarchy machine, from a checkout:

```bash
./install.sh --bind      # or without --bind to just print the keybindings
```

That installs the binary to `~/.local/bin`, stages and validates both plugins into
`~/.config/omarchy/plugins/`, writes the launcher entry and adds the keybindings to
`~/.config/hypr/bindings.lua`. See `omarchy/README.md` and `app/README.md` for settings
and attribution.

## Usage

```bash
rustybui set-location "Leiden" --lat 52.16 --lon 4.49 --country NL --timezone Europe/Amsterdam
rustybui nowcast                 # human-readable table (+ where each answer came from)
rustybui nowcast --json          # the camelCase view contract the QML consumes
rustybui bar                     # Waybar-style JSON for the Omarchy bar: text/class/tooltip
rustybui app                    # one payload for a whole screen: nowcast + forecast + air
rustybui app --text             # …or a short human summary of the same payload
rustybui forecast --hours 12 --days 3
rustybui air
rustybui search Amersfoort
rustybui cache [--clear]         # report or clear the caches
```

Global flags: `--refresh` (ignore the per-cycle nowcast cache), `--offline` (never touch
the network), `--knmi-key` (or `KNMI_API_KEY`).

## Caching and rate limiting

The three open-data endpoints are free and shared, so three layers keep this tool from
being a bad citizen (all in `rustybui-core`):

| Layer | Key | Window |
|-------|-----|--------|
| `DiskCache` | location + 5-minute radar cycle | until the cycle rolls |
| `ResponseCache` | request URL | 300 s fresh, 6 h stale-fallback, `offline()` mode |
| `HttpClient` gate | per provider | 200 ms minimum gap, 60 requests/minute |

Request URLs are kept stable inside their window — KNMI `TIME` and Bright Sky `date` are
snapped to the 5-minute cycle — so a second run inside a cycle is free. A forced refresh
that crosses a cycle costs at most 24 KNMI point queries, five seconds apart at the gate's
pace, and never a burst.

## Data sources and attribution

| Provider   | Used for                        | Licence |
|------------|---------------------------------|---------|
| KNMI       | radar nowcast (NL, +0…+115 min) | CC BY 4.0 |
| DWD (Bright Sky) | forecast + radar nowcast (DE) | CC BY 4.0 |
| Open-Meteo | forecast, air quality, geocoding, minutely-15 nowcast | CC BY 4.0 |

Licence: GPL-3.0-or-later, as a derivative work of WeatherMaster.
