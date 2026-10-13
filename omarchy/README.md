# Rain nowcast — Omarchy 4 bar plugin

A bar pill with the two-hour rain nowcast for one location, plus a detail popout
with the buienradar-style chart. Written for Omarchy 4 ("Quattro"), whose shell is a
single long-running Quickshell process that loads plugins from
`~/.config/omarchy/plugins/<id>/`.

```
manifest.json     plugin contract: one bar-widget, right-hand section by default
Pill.qml          the pill; the shell routes open/close/toggle through it
RainPanel.qml     the popout: chart, summary, provenance
```

`Model.js` and `NowcastChart.qml` come from the repo's `qml/` directory and are copied in
when the plugin is staged — the window plugin under `app/` uses the same two files.

The entry points are not called `BarWidget.qml`/`Panel.qml` on purpose: a component file
named after the type it extends makes the type name ambiguous with the imported one.

## Install

On the Omarchy machine, from a checkout of this repo:

```bash
./install.sh --bind     # build, install both plugins, place the pill, add the keybinding
```

The installer also adds one Hyprland window rule to `~/.config/hypr/hyprland.lua`, so the
weather window floats and opens flush with the bar — the same top edge the nowcast popout
uses — instead of tiling into the workspace. The offset is read from the running bar's height
and the look'n'feel's outer gap, so it does not have to be hand-tuned:

```lua
-- The weather window floats and opens flush with the bar.
o.window({ class = "^org.quickshell$", title = "^Weather$" }, { float = true, move = { "(monitor_w-window_w)/2", "26" } })
```

Without `--bind` the keybinding is only printed, so you can decide where it goes in
`~/.config/hypr/bindings.lua`. The line it adds is:

```lua
o.bind("SUPER + SHIFT + R", "Rain nowcast", { panel = "mmb.rainnowcast" })
```

`SUPER + SHIFT + R` is unbound in Omarchy's default bindings. The panel can also be toggled by
**right-clicking** the pill, refreshed by middle-clicking it, or opened from a shell:

```bash
omarchy-shell shell toggle mmb.rainnowcast
```

A **left click** opens the weather window instead (`omarchy-shell shell toggle mmb.weather '{}'`)
— the pill is the bar's way into both surfaces. Right-click is not reserved by the bar: the
built-in widgets use it the same way.

## Settings

The pill's own pieces are all optional, so it can be as busy or as bare as you like. The label is
built from the binary's payload, so the temperature is already in whatever units are configured
(`rustybui prefs --units`), and a forecast that cannot be had leaves the pill with the rain state
alone.

| Setting | Default | Meaning |
|---------|---------|---------|
| `binaryPath` | `rustybui` | The binary to run; absolute if it is not on the shell's `PATH` |
| `pollMinutes` | `5` | How often the pill asks for a fresh view (radar republishes every 5 min) |
| `clock24` | `true` | 12-hour clock in the popout when false |
| `showIcon` | `true` | The rain/cloud glyph. With `showState` off too, it is the only thing that can say it is dry. |
| `showState` | `true` | The number: the rate while it rains, the wait when it is coming. Nothing when it is dry. |
| `showTemperature` | `true` | The current temperature. |
| `iconSide` | `left` | `left` → `󰖐 21m 12.2°`; `right` → `21m 12.2° 󰖐`. Only matters when the icon is shown. |

Turning every part off leaves the word the summary already carries (`dry`), so the pill never
disappears and stays clickable. The four display settings are editable from Omarchy's own
bar-widget settings — the manifest carries a `schema` for them — or inline in
`~/.config/omarchy/shell.json` under the widget's `bar.layout` entry:

```json
{ "id": "mmb.rainnowcast", "binaryPath": "/home/you/.local/bin/rustybui", "pollMinutes": 5, "clock24": false, "showIcon": true, "showState": true, "showTemperature": true, "iconSide": "left" }
```

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

Weather warnings are DWD's, and they appear in the app window and in `rustybui alerts`: this
popout is about the two hours of rain ahead, and a warning card in it would compete with that.
The location, clock and hours the window edits with `rustybui prefs` are shared with the CLI,
not with this plugin, whose own settings come from Omarchy.

## Checking it

```bash
node test/model.test.mjs                          # payload and label logic, no shell needed
container/check-qml.sh /path/to/omarchy-checkout  # manifests + QML against the real shell
container/render-qml.sh /path/to/omarchy-checkout # render it offscreen; PNGs in test/render/out
```

`check-qml.sh` stages both plugins exactly as `install.sh` does, validates each manifest
with Omarchy's own `omarchy-plugin-validate`, and type-checks the QML against an Omarchy
source tree. Unresolved `Quickshell` imports are expected there (Quickshell is not
installed in the build container) and are stubbed, so the `qs.Commons` and `qs.Ui` members
resolve — which is what catches typos in theme tokens and base-type properties.

`render-qml.sh` goes further and actually runs the QML, against captured payloads, inside a
stubbed shell — which is how the `ReferenceError` in the chart and the missing `Card.qml`
were found. See `test/render/README.md`.
