# rusty-bui — a Nowcast Widget for Omarchy

A rain **nowcast widget for [Omarchy](https://omarchy.org/)** (Hyprland + Quickshell): what will fall from the sky in the **next two hours**, at radar resolution, in the bar, with a detail popout.

Omarchy has plenty of weather widgets — current conditions, forecasts, forecasts again. What it barely has is nowcasting: "is it about to rain on me in the next two hours", which needs weather radar, not a forecast model. So far that existed only for **[Japan](https://plugins.omarchy.org/plugin.html?id=hakatamike.weather)** (JMA radar), the **[Netherlands and Belgium](https://plugins.omarchy.org/plugin.html?id=io.github.dzanaga.omarchy-rain-radar-belgium-netherlands-widget)** (Buienradar) and **[Denmark](https://plugins.omarchy.org/plugin.html?id=io.github.diegogardini.omarchy-rain-radar-denmark-widget)** (DMI radar). This widget serves **Germany's DWD radar**, keeps the **Netherlands' KNMI radar**, and through **Open-Meteo's minutely-15 model** gets you a near-global nowcast everywhere the radars don't reach — the first rain-nowcast widget for Omarchy that works almost anywhere you happen to be. Same story for the full weather window that ships alongside it (current conditions, the same nowcast chart, hourly and daily outlook, air quality and DWD warnings).

If you live under a KNMI or DWD radar you get genuine radar nowcast; everywhere else you get the best model available. Whichever provider answers is named in the payload.

```text
Dry, dry, dry …  ☔ 13.4 mm/h within 26 min
```

## Install

The widget shells out to the `rustybui` binary, which owns all fetching, caching and
rate limiting. Install the binary first, then the pill.

**1. The binary** — either build it:

```bash
git clone https://github.com/Hizoul/rusty-bui.git && cd rusty-bui
./install.sh --no-plugins          # builds rustybui into ~/.local/bin
```

or grab a prebuilt one from [Releases](https://github.com/Hizoul/rusty-bui/releases):

```bash
mkdir -p ~/.local/bin
tar -xzf rustybui-*-x86_64.tar.gz -C ~/.local/bin rustybui && chmod +x ~/.local/bin/rustybui
```

**2. The bar pill** — one command, against the generated plugin repo:

```bash
omarchy plugin add https://github.com/Hizoul/omarchy-weather-nowcast.git --enable
```

Then set your location with `rustybui set-location "Leiden" --lat 52.16 --lon 4.49 --country NL --timezone Europe/Amsterdam` (or leave it: it defaults to De Bilt, and the pill's settings can override the binary path).

**Everything at once, on an Omarchy machine**, from a checkout of this repo:

```bash
./install.sh             # build, install binary + both plugins, launcher entry, float rule
./install.sh --bind      # …and add the keybindings to ~/.config/hypr/bindings.lua
```

The installable single-plugin unit is [Hizoul/omarchy-weather-nowcast](https://github.com/Hizoul/omarchy-weather-nowcast) — generated from this repo by CI on every push (`scripts/stage-plugins.sh` dereferences the shared QML). `omarchy plugin add` clones a repository and validates its **root** manifest, and this dev repo intentionally keeps its full source tree, so the standalone repo is the one the marketplace should point at.

## What you get

Two Omarchy 4 ("Quattro") shell plugins, both thin: they own no fetching, they run `rustybui` and render the result.

- **`mmb.rainnowcast`** — the bar pill and its popout: the two-hour nowcast for the stored location, in the bar and in a popout on click or `SUPER + SHIFT + R`.
- **`mmb.weather`** — the weather window (installed by `install.sh` from this repo, or opened with `omarchy-shell shell toggle mmb.weather '{}'`): current conditions, the same nowcast chart, hourly and daily outlook, air quality.

## Usage

```bash
rustybui set-location "Leiden" --lat 52.16 --lon 4.49 --country NL --timezone Europe/Amsterdam
rustybui nowcast                 # human-readable table (+ where each answer came from)
rustybui nowcast --json          # the camelCase view contract the QML consumes
rustybui bar                     # Waybar-style JSON for the Omarchy bar: text/class/tooltip
rustybui app                     # one payload for a whole screen: warnings + nowcast + forecast + air + sky
rustybui app --text              # …or a short human summary of the same payload
rustybui forecast --hours 12 --days 3
rustybui day --date 2026-10-12   # one day, opened up: the rows behind a day in the list
rustybui alerts                  # warnings in force, worst first
rustybui radar --json            # DWD's radar composite as a georeferenced PNG, for the map
rustybui air
rustybui search Amersfoort
rustybui prefs                   # read or change what the window shows: clock, hours, offline, units
rustybui places [--add] [--use <name>] [--remove <name>]   # the places you switch between
rustybui cache [--clear]         # report or clear the caches
```

Global flags: `--refresh` (ignore the per-cycle nowcast cache), `--offline` (never touch the network), `--knmi-key` (or `KNMI_API_KEY`).

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
.github/workflows/     plugin mirror + prebuilt release binaries
install.sh             build, install both plugins, launcher entry, float rule, keybindings
```

## Building and testing (podman)

The host has no toolchain; everything runs in a container matching Omarchy's Arch base.

```bash
container/build.sh                             # build the image
container/run.sh cargo test                    # unit tests (offline fixtures)
container/run.sh cargo test -p rustybui-core --test live_nowcast -- --ignored --test-threads=1
container/run.sh cargo run -p rustybui-cli -- nowcast --lat 52.16 --lon 4.49
```

Live tests hit the real endpoints, so they share one persistent cache directory (`.cache/live-tests`, git-ignored): a repeat run inside the same radar cycle sends no requests at all.

The QML is checked without a running shell:

```bash
node test/model.test.mjs                          # payload and label logic
container/check-qml.sh /path/to/omarchy-checkout  # manifests + QML against the real shell
container/render-qml.sh /path/to/omarchy-checkout # render offscreen: PNGs in test/render/out
```

The last one runs the real QML against captured payloads inside a stubbed shell and writes images, which is how a `ReferenceError` in the chart and an unresolved `Card` were caught. Caveats and stub fidelity: `test/render/README.md`.

## Releases and the plugin mirror

Two workflows run on this repo:

- **`publish-plugin-mirror.yml`** — on every push to `main`, stages the pill (`scripts/stage-plugins.sh`) and pushes manifest + QML to [Hizoul/omarchy-weather-nowcast](https://github.com/Hizoul/omarchy-weather-nowcast), the repo the official plugin catalog installs from. Needs a `MIRROR_TOKEN` secret with contents-write access to the mirror repo.
- **`release.yml`** — on a `v*` tag, builds `rustybui` for x86_64 and aarch64 Linux and attaches tarballs to the GitHub Release.

Bump the version in three places per release: `Cargo.toml` (`workspace.package.version`) and both plugin `manifest.json` files (the mirror is regenerated from `omarchy/manifest.json` on push).

## Caching and rate limiting

The open-data endpoints used here are free and shared, so three layers keep this tool from being a bad citizen (all in `rustybui-core`):

| Layer | Key | Window |
|-------|-----|--------|
| `DiskCache` | location + 5-minute radar cycle | until the cycle rolls |
| `ResponseCache` | request URL | 300 s fresh, 6 h stale-fallback, `offline()` mode |
| `HttpClient` gate | per provider | 200 ms minimum gap, 60 requests/minute |

Request URLs are kept stable inside their window — KNMI `TIME` and Bright Sky `date` are snapped to the 5-minute cycle — so a second run inside a cycle is free. A forced refresh that crosses a cycle costs at most 24 KNMI point queries, five seconds apart at the gate's pace, and never a burst.

## Data sources and attribution

Three providers, and each one is asked for what it reports on rather than treated as a uniform backend:

| | nowcast (next 2 h) | forecast | warnings |
|---|---|---|---|
| **KNMI** (NL radar) | preferred inside the Netherlands | — | — (keyed dataset only) |
| **DWD** (Bright Sky) | preferred elsewhere | preferred inside Germany | the CAP messages, keylessly |
| **Open-Meteo** | model fallback, and everywhere radars do not reach | preferred everywhere else | — |

Whichever provider fails falls back to the next, and the payload says who answered. DWD's radar, forecast and warnings share one politeness gate and one set of counters.

Warnings are DWD's, which is what a keyless source offers: KNMI publishes Dutch warnings only through a keyed dataset on the KNMI Data Platform, so the Netherlands shows no warnings rather than an invented one. Attribution (CC BY 4.0) is in every payload a front-end renders.

| Provider | Used for | Licence |
|----------|----------|---------|
| KNMI | radar nowcast (NL, +0…+115 min) | CC BY 4.0 |
| DWD (Bright Sky) | forecast + radar nowcast (DE) | CC BY 4.0 |
| Open-Meteo | forecast, air quality, geocoding, minutely-15 nowcast | CC BY 4.0 |

## Credits and licence

Rust port of [WeatherMaster](https://github.com/PranshulGG/WeatherMaster) (Android/Compose, GPL-3.0) by PranshulGG: this project ports its data pipeline to Rust and replaces the Android UI with Omarchy shell plugins. Licence: **GPL-3.0-or-later**, as a derivative work.