#!/usr/bin/env bash
# Render the plugin QML headlessly and write PNGs. Runs inside the build container;
# container/render-qml.sh is the host-side entry point.
#
#   run.sh <omarchy-checkout> [dry|wet]
#
# Assembles a stub shell in /tmp (Quickshell from test/render/stubs, qs.Commons copied
# from the checkout so the real theme token code is exercised), feeds the drivers the
# captured payloads, renders offscreen with the software backend, and reports the images.
set -euo pipefail

omarchy="${1:?usage: run.sh <omarchy-tree> [dry|wet]}"
case_name="${2:-dry}"
repo=/work
work=/tmp/render
out=$repo/test/render/out
qml_bin="$(command -v qml || true)"
[[ -n $qml_bin ]] || qml_bin=/usr/lib/qt6/bin/qml

[[ -d $omarchy/shell/Commons ]] || {
  echo "not an Omarchy source tree: $omarchy" >&2
  exit 2
}

rm -rf "$work"
mkdir -p "$work/Quickshell/Io" "$work/Quickshell/Hyprland" "$work/qs/Ui" "$work/qs/Commons" "$out"

# Stubs we own.
cp "$repo"/test/render/stubs/Quickshell/qmldir "$repo"/test/render/stubs/Quickshell/*.qml "$work/Quickshell/"
cp "$repo"/test/render/stubs/Quickshell/Io/* "$work/Quickshell/Io/"
cp "$repo"/test/render/stubs/Quickshell/Hyprland/* "$work/Quickshell/Hyprland/"
cp "$repo"/test/render/stubs/qs/Ui/* "$work/qs/Ui/"

# Theme tokens: the shell's own files, so the render is themed by the code that themes
# the shell. Their Quickshell imports are satisfied by the stubs above.
for file in Color.qml Style.qml Border.qml Util.qml BorderGeometry.js; do
  cp "$omarchy/shell/Commons/$file" "$work/qs/Commons/$file"
done
cat >"$work/qs/Commons/qmldir" <<'QMLDIR'
module qs.Commons
singleton Color 1.0 Color.qml
singleton Style 1.0 Style.qml
singleton Border 1.0 Border.qml
singleton Util 1.0 Util.qml
QMLDIR

# Payloads the binary produces, as a JS library the Process stub serves.
# The day fixture is the same for every case: it is what an opened row asks for.
day_fixture=day-leiden.json
case "$case_name" in
  dry)
    bar_fixture=bar-dry.json
    app_fixture=app-leiden.json
    drivers="App: AppSettings:settings Pill:"
    ;;
  wet)
    bar_fixture=bar-wet-synthetic.json
    app_fixture=app-leiden.json
    drivers="App: AppSettings:settings Pill:"
    ;;
  alerts)
    # The captured app payload was taken on a quiet day; this case adds warnings so the
    # card that shows them can be rendered and looked at.
    bar_fixture=bar-wet-synthetic.json
    app_fixture=app-alerts-synthetic.json
    drivers="App: AppSettings:settings Pill:"
    ;;
  map)
    # The rain map: the only card that fetches for itself, so it is rendered on its own.
    bar_fixture=bar-dry.json
    app_fixture=app-leiden.json
    drivers="Map:"
    ;;
  daylight)
    # The captured payload was taken after sunset, when the sky card is right to leave its
    # daylight bar out: this case moves the clock back to the afternoon so the bar can be seen.
    bar_fixture=bar-dry.json
    app_fixture=app-daylight-synthetic.json
    drivers="App: AppSettings:settings Pill:"
    ;;
  day)
    # One row of the day list opened up. It cannot be reached by rendering the window: a day
    # detail only exists after a click, so this case's driver clicks one.
    bar_fixture=bar-dry.json
    app_fixture=app-leiden.json
    drivers="AppDay:"
    ;;
  keys)
    # The key list `?` opens, and the keys around it. Same reason as `day`: only a press can show
    # it, so this case's driver presses them.
    bar_fixture=bar-dry.json
    app_fixture=app-leiden.json
    drivers="AppKeys:"
    ;;
  layout)
    # The cards moved by key, by drag and by the chips, then the layout a stored preference
    # produces. Only calls can show it, so this case's driver makes them.
    bar_fixture=bar-dry.json
    app_fixture=app-leiden.json
    drivers="AppLayout:"
    ;;
  *)
    echo "unknown case: $case_name (expected dry, wet, alerts, daylight, day, keys, layout or map)" >&2
    exit 2
    ;;
esac
# A synthesized radar frame (test/fixtures/make-radar-png.py) and the frames the service
# would have listed around now, in the shape `rustybui radar --json` prints them.
python3 - "$repo/test/fixtures" >"$work/radar.json" <<'PY'
import json, pathlib, sys, time
fixtures = pathlib.Path(sys.argv[1])
step = 5 * 60 * 1000
now = int(time.time() * 1000)
frames = [now - 18 * step + index * step for index in range(24)]
print(json.dumps({
    "image": {
        "path": str(fixtures / "radar-sample.png"),
        "bbox": [2.73, 51.49, 6.25, 52.83],
        "width": 400,
        "height": 250,
        "timeMs": frames[18],
        "attribution": "Radar © DWD",
        "cached": True,
    },
    "framesMs": frames,
    "attribution": "Radar © DWD",
}))
PY
{
  printf '.pragma library\n\n'
  # JSON is a subset of JS, so the payloads can be inlined verbatim.
  printf 'var app = '
  cat "$repo/test/fixtures/$app_fixture"
  printf '\n\nvar bar = '
  cat "$repo/test/fixtures/$bar_fixture"
  printf '\n\nvar radar = '
  cat "$work/radar.json"
  printf '\n\nvar day = '
  cat "$repo/test/fixtures/$day_fixture"
  printf '\n\n'
  # `rustybui prefs --json` and `rustybui search … --json`, captured from live runs. One place
  # is saved (Berlin, which is *not* the stored one) so the settings card renders both halves of
  # the places row: a chip to switch to, and a chip to save the place currently shown.
  printf 'var prefs = {"clock24":true,"hours":24,"offline":false,"units":"metric","durationStyle":"colon","cardOrder":["now","sky","nowcast","map","hours","days","air"],"hiddenCards":[],"stored":{"clock":false,"hours":false,"offline":false,"units":false,"duration":false,"cards":false},"location":{"name":"Leiden","latitude":52.16,"longitude":4.49,"country_code":"NL","timezone":"Europe/Amsterdam"},"places":[{"name":"Berlin","latitude":52.52,"longitude":13.405,"country_code":"DE","timezone":"Europe/Berlin"}],"placeLimit":12}\n'
  printf '\nvar search = [{"name":"Leiden, South Holland","latitude":52.15833,"longitude":4.49306,"country_code":"NL","timezone":"Europe/Amsterdam"},{"name":"Leidenborn, Rhineland-Palatinate","latitude":50.13333,"longitude":6.21667,"country_code":"DE","timezone":"Europe/Berlin"}]\n'
} >"$work/Quickshell/Io/Fixture.js"

export QT_QPA_PLATFORM=offscreen
export QT_QUICK_BACKEND=software
# Without this, Qt sends QML errors to the journal instead of the console and the whole
# run looks like a silent, unexplained "Did not load any objects" — which is how the first
# version of this harness wasted half an hour.
export QT_FORCE_STDERR_LOGGING=1
export QT_LOGGING_RULES='qt.qpa.*=false'

status=0
# name:prefix — the prefix keeps the two app-window renders apart.
for driver in $drivers; do
  name="${driver%%:*}"
  prefix="${driver#*:}"
  echo "== $name.qml (payload case: $case_name)"
  rm -f "$out"/*.png
  if ! timeout 120 "$qml_bin" -I "$work" "$repo/test/render/drivers/$name.qml" 2>&1 |
    sed "s/^/   /"; then
    status=1
  fi
  if compgen -G "$out/*.png" >/dev/null; then
    # Keep each case's images, so dry and wet can be looked at side by side.
    mkdir -p "$out/$case_name"
    mv "$out"/*.png "$out/$case_name/"
    python "$repo/test/render/png-stats.py" "$out/$case_name"/*.png || status=1
  else
    echo "   !! no image written — the driver never reached Harness.grabAll()"
    status=1
  fi
done

echo
echo "images in $out (on the host: test/render/out/)"
exit $status
