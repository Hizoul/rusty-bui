#!/usr/bin/env bash
# Render the plugin QML headlessly: the real App.qml / Pill.qml / RainPanel.qml, driven by
# captured payloads, inside a stubbed shell, offscreen. Writes PNGs to test/render/out/.
#
#   container/render-qml.sh /path/to/omarchy-checkout [dry|wet]
#
# Use this to check the layout actually instantiates and looks sane — qmllint proves the
# types and properties exist, this proves the bindings evaluate and the thing draws. The
# stubs are in test/render/stubs and the caveats are in test/render/README.md.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
omarchy="${1:-${OMARCHY_PATH:-}}"
case_name="${2:-dry}"

if [[ -z $omarchy || ! -d $omarchy/shell/Commons ]]; then
  echo "usage: container/render-qml.sh /path/to/omarchy-checkout [dry|wet]" >&2
  exit 2
fi

exec podman run --rm -i \
  --userns=keep-id \
  -v "$root":/work:z \
  -v "$omarchy":/omarchy-src:ro \
  -v rustybui-cache:/cache \
  -w /work \
  rustybui-dev:latest sh -c "exec sh /work/test/render/run.sh /omarchy-src $case_name"
