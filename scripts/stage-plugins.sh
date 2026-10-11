#!/usr/bin/env bash
# Copy each plugin, plus the shared QML it needs, into <destination>, dereferencing
# the symlinks the repo keeps for editors and qmllint.
#
#   scripts/stage-plugins.sh /tmp/staged
#
# Omarchy rejects symlinks inside a plugin folder, so this staged tree — not the
# working tree — is what install.sh installs and what container/check-qml.sh
# validates.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
dest="${1:-}"
if [[ -z $dest ]]; then
  echo "usage: scripts/stage-plugins.sh <destination-directory>" >&2
  exit 2
fi

for id in mmb.rainnowcast mmb.weather; do
  rm -rf "${dest:?}/$id"
  mkdir -p "$dest/$id"
done

# The bar pill and its popout.
cp -L "$root/omarchy/manifest.json" "$root/omarchy/Pill.qml" "$root/omarchy/RainPanel.qml" \
  "$dest/mmb.rainnowcast/"
cp -L "$root/qml/Model.js" "$root/qml/NowcastChart.qml" "$dest/mmb.rainnowcast/"

# The standalone window.
cp -L "$root/app/manifest.json" "$root/app/App.qml" "$dest/mmb.weather/"
cp -L "$root/qml/Model.js" "$root/qml/NowcastChart.qml" "$root/qml/Card.qml" "$dest/mmb.weather/"

if [[ -n $(find "$dest" -type l -print -quit) ]]; then
  echo "staging left a symlink behind; Omarchy will reject that plugin" >&2
  exit 1
fi

echo "staged mmb.rainnowcast and mmb.weather into $dest"
