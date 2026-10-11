# Headless render harness

`qmllint` proves the types exist. This proves the QML actually loads and draws: it renders
the real `app/App.qml`, `omarchy/Pill.qml` and `omarchy/RainPanel.qml` offscreen, with the
real payloads the binary produces, and writes PNGs to `test/render/out/`.

```bash
container/render-qml.sh /path/to/omarchy-checkout        # dry: nothing falling in the window
container/render-qml.sh /path/to/omarchy-checkout wet    # rain in about 20 minutes
```

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

Both of these were real, and both were invisible to `qmllint`:

- `qml/NowcastChart.qml` read `tickRow.height` from an id inside a `Repeater` delegate, which
  is a `ReferenceError` on every evaluation, so the tick row never had a height. It now
  measures the font.
- `app/App.qml` uses `Card`, and the repo's `app/` directory had no `Card.qml` for it to
  resolve. The staging script copies it into the plugin, so what shipped was fine and the
  staged `qmllint` was happy — but editors, and this harness, saw a directory where `Card`
  was undefined. Hence the symlink, and hence the `is not a type` guard in
  `container/lint-plugins.sh` that would catch a staging script that does drop a file.

Two more looked like bugs and were not: `implicitHeight` and `minimumSize` on
`FloatingWindow` are real, Omarchy's own gallery sets both, and it was this harness's stub
that was missing them.
