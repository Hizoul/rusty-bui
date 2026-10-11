# Rain nowcast — Omarchy 4 bar plugin

A bar pill with the two-hour rain nowcast for one location, plus a detail popout
with the buienradar-style chart. Written for Omarchy 4 ("Quattro"), whose shell is a
single long-running Quickshell process that loads plugins from
`~/.config/omarchy/plugins/<id>/`.

```
manifest.json     plugin contract: one bar-widget, right-hand section by default
BarWidget.qml     the pill; the shell routes open/close/toggle through it
Panel.qml         the popout: chart, summary, provenance
NowcastChart.qml  the chart itself (hour ticks, "now" marker, mm/h axis)
Model.js          payload parsing and label formatting, no QML types
test/             node checks of Model.js against a captured payload
install.sh        build + install on an Omarchy machine
```

## Install

On the Omarchy machine, from a checkout of this repo:

```bash
omarchy/install.sh --bind     # build, install, place the widget, add SUPER + SHIFT + R
```

Without `--bind` the keybinding is only printed, so you can decide where it goes in
`~/.config/hypr/bindings.lua`. The line it adds is:

```lua
o.bind("SUPER + SHIFT + R", "Rain nowcast", { panel = "mmb.rainnowcast" })
```

`SUPER + SHIFT + R` is unbound in Omarchy's default bindings. The panel can also be
toggled by clicking the pill, by middle-clicking it to refresh, or from a shell:

```bash
omarchy-shell shell toggle mmb.rainnowcast
```

## Settings

Defaults live in `manifest.json`; override them inline in `~/.config/omarchy/shell.json`
under the widget's `bar.layout` entry:

```json
{ "id": "mmb.rainnowcast", "binaryPath": "/home/you/.local/bin/rustybui", "pollMinutes": 5, "clock24": false }
```

| Setting | Default | Meaning |
|---------|---------|---------|
| `binaryPath` | `rustybui` | The binary to run; absolute if it is not on the shell's `PATH` |
| `pollMinutes` | `5` | How often the pill asks for a fresh view (radar republishes every 5 min) |
| `clock24` | `true` | 12-hour clock in the popout when false |

Move it elsewhere on the bar with `omarchy bar move mmb.rainnowcast --section left`.

## Where the data comes from

The plugin owns no fetching. It runs `rustybui bar --json` and renders the result;
the binary holds the cache, the politeness gate and the fallback chain:

1. **KNMI** `radar_forecast_2.0` via WMS — 5-minute radar frames, the Netherlands,
   +0…+115 minutes.
2. **DWD** Radarkomposit RV via Bright Sky — 5-minute radar frames, Germany.
3. **Open-Meteo** `minutely_15` — model precipitation, worldwide, 15-minute steps
   (drawn as 15-minute bars: the popout says "model" instead of "radar").

Because the binary caches per 5-minute radar cycle, a pill that polls every 5 minutes
sends at most one round of requests per cycle, and usually none at all. `--offline`
serves the last fetch without touching the network.

Attribution is rendered in the popout: **Radar © KNMI**, **Radar © DWD (Bright Sky)**
or **Model © Open-Meteo**, all CC BY 4.0. The location is whatever
`rustybui set-location` stored (`rustybui which` shows it).

## Checking it

```bash
node omarchy/test/model.test.mjs                    # payload/label logic, no shell needed
container/check-qml.sh /path/to/omarchy-checkout    # manifest + QML against the real shell
```

`check-qml.sh` validates the manifest with Omarchy's own `omarchy-plugin-validate` and
type-checks the QML against an Omarchy source tree. Unresolved `Quickshell` imports are
expected there (Quickshell is not installed in the build container); the `qs.Commons`
and `qs.Ui` members resolve, which is what catches typos in theme tokens and base-type
properties.
