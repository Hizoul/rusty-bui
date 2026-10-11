#!/usr/bin/env bash
# Check both plugins the way Omarchy will load them: stage them exactly as
# install.sh does, validate each manifest with Omarchy's own checker, then
# type-check the QML against the real shell sources.
#
#   container/check-qml.sh /path/to/omarchy-checkout
#
# The checkout only needs its shell/ directory (for `qs.Commons`, `qs.Ui`) and bin/
# (the manifest validator). The checking itself lives in container/lint-plugins.sh,
# which runs inside the container — written as an ordinary script rather than a
# quoted string, which is what a here-doc-heavy inline version kept getting wrong.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
omarchy="${1:-${OMARCHY_PATH:-}}"

if [[ -z $omarchy || ! -d $omarchy/shell ]]; then
  echo "usage: container/check-qml.sh /path/to/omarchy-checkout   (needs shell/ and bin/)" >&2
  exit 2
fi

exec podman run --rm -i \
  --userns=keep-id \
  -v "$root":/work:z \
  -v "$omarchy":/omarchy-src:ro \
  -v rustybui-cache:/cache \
  -w /work \
  rustybui-dev:latest sh -c '
    set -eu
    echo "== staging both plugins (symlinks dereferenced, exactly as install.sh does)"
    sh /work/scripts/stage-plugins.sh /tmp/staged >/dev/null
    exec sh /work/container/lint-plugins.sh /omarchy-src /tmp/staged/mmb.rainnowcast /tmp/staged/mmb.weather
  '
