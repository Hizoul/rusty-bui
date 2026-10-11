# Weather — the standalone window

A weather window for Omarchy 4, opened from the launcher, from the Omarchy menu, from a
keybinding, or from a shell:

```bash
omarchy-shell shell toggle mmb.weather '{}'
```

It is a `panel`-kind plugin, so it lives inside the long-running `omarchy-shell`
process — no second Quickshell instance, and it inherits the active theme for free.

```
manifest.json   plugin contract: one panel, kept loaded between summons
App.qml         the window: masthead, now, rain nowcast, hours, days, air
Card.qml        (shared, from qml/) the themed surface each block sits on
```

## What it shows

One payload — `rustybui app` — drives every block, so the window costs one process spawn
and one round of cache-aware requests:

- **Now** — temperature, conditions, feels-like, and a six-cell grid (wind, humidity,
  pressure, cloud cover, UV, rain).
- **Rain nowcast** — the same `NowcastChart.qml` the bar popout draws: one bar per radar
  frame, hour ticks, a now marker, mm/h axis, plus the sentence and the status line.
- **Next hours** — a scrollable strip with the glyph, temperature and precipitation
  probability per hour.
- **Days** — up to seven rows with the day's glyph, rain, UV and low/high.
- **Air quality** — the European AQI band, coloured from the theme, with PM2.5 and PM10.
- **Provenance and failures** — the CC BY 4.0 attribution, and any part that failed
  (`rustybui` reports per-source errors rather than hiding a block).

`R` or `F5` refreshes, `Esc` closes.

## Settings

Defaults live in `manifest.json`; override them inline in `~/.config/omarchy/shell.json`
under the plugin's `plugins[]` entry:

```json
{ "id": "mmb.weather", "binaryPath": "/home/you/.local/bin/rustybui", "refreshMinutes": 10, "hours": 24, "clock24": false, "offline": true }
```

| Setting | Default | Meaning |
|---------|---------|---------|
| `binaryPath` | `rustybui` | The binary to run; absolute if it is not on the shell's `PATH` |
| `refreshMinutes` | `10` | How often the window refetches while open |
| `hours` | `24` | Hourly rows to fetch |
| `clock24` | `true` | 12-hour clock when false |
| `offline` | `false` | Never touch the network; use the last cached fetch |

## Where the data comes from

The window fetches nothing itself. `rustybui app` owns the chain — KNMI radar for the
Dutch nowcast, DWD/Bright Sky for Germany, Open-Meteo for the forecast, air quality and
the worldwide `minutely_15` nowcast fallback — plus the caches and the rate limiting.
Attribution is rendered in the footer; all three sources are CC BY 4.0.

## Checking it

```bash
node test/model.test.mjs                          # payload and label logic, no shell needed
container/check-qml.sh /path/to/omarchy-checkout  # manifest + QML against the real shell
```

`qmllint` cannot resolve `Quickshell` in the build container, so a stub stands in for it;
without that stub the `KeyboardPanel`-style entry points report spurious
"missing required property" warnings. See `container/lint-plugins.sh`.
