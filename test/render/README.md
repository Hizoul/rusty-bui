# Headless render harness

`qmllint` proves the types exist. This proves the QML actually loads and draws: it renders
the real `app/App.qml`, `omarchy/Pill.qml` and `omarchy/RainPanel.qml` offscreen, with the
real payloads the binary produces, and writes PNGs to `test/render/out/`.

```bash
container/render-qml.sh /path/to/omarchy-checkout        # dry: nothing falling in the window
container/render-qml.sh /path/to/omarchy-checkout wet    # rain in about 20 minutes
container/render-qml.sh /path/to/omarchy-checkout alerts  # warnings in force
container/render-qml.sh /path/to/omarchy-checkout daylight # the sun still up, the moon a crescent
container/render-qml.sh /path/to/omarchy-checkout day    # a day in the list opened up
container/render-qml.sh /path/to/omarchy-checkout map    # the radar map on its own
```

Three drivers run against whichever case is asked for: `App.qml` (the window), `AppSettings.qml`
(the same window with its settings open, grabbing with a `settings-` prefix so the two images
stay apart) and `Pill.qml` (the bar pill and the popout it opens); three cases have drivers of
their own, because what they show cannot be reached by rendering the window alone — `AppDay.qml`
clicks a day row to open it, `AppKeys.qml` presses the window's keys and photographs the list
`?` opens, and `Map.qml` renders the map, which is the one card that fetches for itself. That
driver grabs three times rather than twice, because the map plays on its own and one image cannot
show a loop: where it opens (the oldest frame), where it has got to a few seconds later with the
picture keeping up with the frame asked for, and the far end of the extent, where the frame's age
is on the stepper.
The fixtures come from `test/fixtures/`, and the payloads have synthetic variants for
states a quiet day cannot show (`make-bar-wet.py`, `make-app-alerts.py`, `make-app-daylight.py`
for a sun that is still up and a moon big enough to see). Two more are generators rather than
fixtures: `make-basemap.py` writes `qml/Basemap.js` from Natural Earth, and
`make-radar-legend.py` writes `qml/Legend.js` from the ramp DWD publishes — both take `--probe`
or write the file, and both are re-run rather than hand-edited.

It needs an Omarchy checkout for two reasons: `shell/Commons/{Color,Style,Border,Util}.qml`
are copied in as the theme singletons — so the render is themed by the code that themes the
shell, not by an imitation of it — and the PNGs are written into the repo, which is mounted
into the container.

## How it works

`test/render/run.sh` (inside the container) assembles `/tmp/render`: the stub modules from
`test/render/stubs/`, the shell's own `Commons` files, and `Fixture.js`, which holds the
captured payloads (`test/fixtures/`) as JS object literals — JSON being a subset of JS, they
are inlined verbatim. Then it runs the drivers in `test/render/drivers/` with the `qml` tool
on the offscreen platform and the software renderer.

Two stubs matter and are the only ones doing anything clever:

- `Process` answers any command naming the binary from `Fixture.js` — so no build and no
  network are needed, and the plugins see exactly the bytes the binary emitted when the
  payload was captured. Every other command (the shell's own `hyprctl` calls) exits
  non-zero with no output, which is the "tool missing" path those callers already handle.
- `Harness` is a window registry: the stub windows register themselves, the driver calls
  `Harness.grabAll()` once the payload has landed, and `contentItem.grabToImage()` writes
  each one.

## What it is not

The stubs are not the shell. They are faithful only where these three files touch them:
window sizing, the bar handle, the panel controller, the key-catcher signals, and the theme
tokens. A green run means "our QML loads, evaluates and lays out"; it does not mean the
plugin is wired into Omarchy correctly, which only the real shell can show. When a stub
disagrees with the real API, the stub is what is wrong — Omarchy's `dev-gallery` is the
reference for what a `FloatingWindow` accepts.

## What it has already caught

All of these were real, and all were invisible to `qmllint`:

- `qml/NowcastChart.qml` read `tickRow.height` from an id inside a `Repeater` delegate, which
  is a `ReferenceError` on every evaluation, so the tick row never had a height. It now
  measures the font.
- `app/App.qml` uses `Card`, and the repo's `app/` directory had no `Card.qml` for it to
  resolve. The staging script copies it into the plugin, so what shipped was fine and the
  staged `qmllint` was happy — but editors, and this harness, saw a directory where `Card`
  was undefined. Hence the symlink, and hence the `is not a type` guard in
  `container/lint-plugins.sh` that would catch a staging script that does drop a file.
- `qml/RadarMap.qml` coloured its basemap with `Model.withAlpha(map.theme.foreground, 0.5)`,
  and the map's *default* palette is colour *strings*. `Qt.rgba(undefined, …)` is an invalid
  colour, the canvas drops it without a word, and the coastlines were drawn in black on a
  near-black background. The render showed a map with nothing on it but the radar.
- The same file's basemap was a `Canvas`, which renders into a texture — and this harness's
  offscreen software renderer does not composite textures at all. A probe that filled a red
  square proved it: zero red pixels. The basemap is now a `Shape` with `PathSvg`, which is
  geometry, and it appears in the renders.
- `qml/MoonDisc.qml` used `Style.space(26)` without importing `qs.Commons`. The moon simply
  was not there; the text beside it was.

Two more looked like bugs and were not: `implicitHeight` and `minimumSize` on
`FloatingWindow` are real, Omarchy's own gallery sets both, and it was this harness's stub
that was missing them.

## The lesson, in one line

Look at the image, and then check the pixels. Three of the five above were found by cropping
the PNG and reading it, and the two that a look would only have called "a bit odd" — a canvas
that silently drew nothing, and an accent colour that fell back to grey — were settled by
counting pixels of a known colour.
