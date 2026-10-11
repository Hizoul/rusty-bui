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
case "$case_name" in
  dry)
    bar_fixture=bar-dry.json
    app_fixture=app-leiden.json
    ;;
  wet)
    bar_fixture=bar-wet-synthetic.json
    app_fixture=app-leiden.json
    ;;
  alerts)
    # The captured app payload was taken on a quiet day; this case adds warnings so the
    # card that shows them can be rendered and looked at.
    bar_fixture=bar-wet-synthetic.json
    app_fixture=app-alerts-synthetic.json
    ;;
  *)
    echo "unknown case: $case_name (expected dry, wet or alerts)" >&2
    exit 2
    ;;
esac
{
  printf '.pragma library\n\n'
  # JSON is a subset of JS, so the payloads can be inlined verbatim.
  printf 'var app = '
  cat "$repo/test/fixtures/$app_fixture"
  printf '\n\nvar bar = '
  cat "$repo/test/fixtures/$bar_fixture"
  printf '\n\n'
  # `rustybui prefs --json` and `rustybui search … --json`, captured from live runs.
  printf 'var prefs = {"clock24":true,"hours":24,"offline":false,"units":"metric","stored":{"clock":false,"hours":false,"offline":false,"units":false},"location":{"name":"Leiden","latitude":52.16,"longitude":4.49,"country_code":"NL","timezone":"Europe/Amsterdam"}}\n'
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
for driver in App: AppSettings:settings Pill:; do
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
