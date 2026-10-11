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
App.qml         the window: masthead, now, sun and moon, rain nowcast, map, hours, days, air
Card.qml        (shared, from qml/) the themed surface each block sits on
HourStrip.qml   (shared, from qml/) a row of hours — the one card the day detail shares
RadarMap.qml    (shared, from qml/) the pannable, zoomable radar map
Legend.js       (generated) DWD's own rain-rate ramp, from the legend their service publishes
MoonDisc.qml    (shared, from qml/) the moon, drawn from two arcs
Basemap.js      (generated) Natural Earth coastlines and borders, projected at runtime
```

## What it shows

One payload — `rustybui app` — drives every block, so the window costs one process spawn
and one round of cache-aware requests:

- **Warnings** — the CAP messages DWD has in force, worst first, tinted with the theme's
  urgent role. Only there when there is something to say.
- **Now** — temperature, conditions, feels-like, and a six-cell grid (wind, humidity,
  pressure, cloud cover, UV, rain).
- **Sun and moon** — sunrise to sunset, how long the light lasts, how much of it is left, and
  the moon: its phase drawn as a disc, not looked up in a font, with the lit fraction beside it.
  The sun's times are the provider's where it has them and computed where it does not (every
  DWD day); the moon is always computed.
- **Rain nowcast** — the same `NowcastChart.qml` the bar popout draws: one bar per radar
  frame, hour ticks, a now marker, mm/h axis, plus the sentence and the status line.
- **Rain map** — DWD's radar composite over a vendored coastline, drag to move it, wheel to
  zoom, and a stepper that walks the frames from an hour back to the end of the nowcast. The
  stepper's clock carries how far off that frame is ("18:38 · in 25 min"), since the extent runs
  both ways from now, and a legend in the corner gives DWD's own rain-rate bands in their own
  colours — read from the legend the service publishes rather than redrawn from memory, so the
  two cannot disagree.
- **Next hours** — a scrollable strip with the glyph, temperature and precipitation
  probability per hour. Day and night glyphs come from the computed sun times, not a clock
  window.
- **Days** — up to seven rows with the day's glyph, rain, UV and low/high. Clicking a row opens
  that day up: its own figures, then its own 24 hours, in the card below the list. Clicking it
  again — or **Close** — puts it away. Opening a day moves the window to it, because a row near
  the end of the list opens a card that starts below the window.
- **Air quality** — the European AQI band, coloured from the theme, with PM2.5 and PM10.
- **Provenance and failures** — the CC BY 4.0 attribution, and any part that failed
  (`rustybui` reports per-source errors rather than hiding a block).

`R` or `F5` refreshes, `Esc` closes.

## Settings

The window's own settings — the location it shows, the clock, how many hours, offline — are
stored by the binary, in the same file the CLI reads and writes:

```bash
rustybui prefs                  # read them
rustybui prefs --clock 12h      # …or change one
rustybui prefs --json           # what a front-end reads, including which ones are set
```

They can equally be changed from inside the window (the **Settings** affordance in the
masthead), which is the point: searching for a place, storing it and switching the clock all
go through `set-location` and `prefs`, so the window and the command line can never disagree.
The plugin's own `settings` — `binaryPath`, `refreshMinutes` — come from Omarchy and act as
defaults for anything the user has not chosen here.

## Saved places

The settings card carries a **Places** row: one chip per saved place, the current one outlined,
and a `+ Save <place>` chip for the place being shown. Clicking a chip switches to it; the `×`
on a chip forgets it. Underneath, the same list is the CLI's:

```bash
rustybui places                 # list them, » marking the one being shown
rustybui places --add           # save the stored location
rustybui places --use berlin    # switch to a saved place, case-insensitively
rustybui places --remove Berlin # forget one
rustybui places --json          # { places: [...], limit, location }
```

Places are matched by name, and a place arriving twice replaces the one already saved rather
than doubling it. Twelve is the limit, and going past it is refused by name rather than
enforced by quietly dropping one. Forgetting a place from the window has no undo — deliberate,
but it is why the × is a small target on the right of the chip and not the chip itself.


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

## One day, opened up

Clicking a row in the day list asks the binary for that one day and shows its hours underneath:

```bash
rustybui day                    # today, as the JSON the window renders
rustybui day --date 2026-10-12  # a particular day
rustybui day --date 2026-10-12 --text   # …and a table to read in a terminal
```

It is the same provider choice as the forecast — DWD inside Germany, Open-Meteo elsewhere, each
falling back to the other — asked for a single date. A day that has already happened is kept for
a day rather than re-fetched every time the row is reopened; today's is asked for again. The
window's day card and the CLI's `day` print the same payload, so the two cannot drift.

## Where the data comes from

The window fetches nothing itself. `rustybui app` owns the chain — KNMI radar for the
Dutch nowcast, DWD/Bright Sky for Germany, Open-Meteo for the forecast, air quality and
the worldwide `minutely_15` nowcast fallback — plus the caches and the rate limiting.
Attribution is rendered in the footer; all three sources are CC BY 4.0.

## Checking it

```bash
node test/model.test.mjs                          # payload and label logic, no shell needed
container/check-qml.sh /path/to/omarchy-checkout  # manifest + QML against the real shell
container/render-qml.sh /path/to/omarchy-checkout # render it offscreen; PNGs in test/render/out
```

`render-qml.sh` takes an optional payload case: `dry` (nothing falling), `wet` (rain in
about 20 minutes) or `alerts` (warnings in force). It renders three views — the window, the
window with its settings open, and the bar pill plus popout — and `test/render/png-stats.py`
fails if an image comes out blank.

`qmllint` cannot resolve `Quickshell` in the build container, so a stub stands in for it;
without that stub the `KeyboardPanel`-style entry points report spurious
"missing required property" warnings. See `container/lint-plugins.sh`.
