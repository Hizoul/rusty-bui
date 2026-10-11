#!/usr/bin/env bash
# Install the rain-nowcast plugin on an Omarchy machine.
#
#   omarchy/install.sh                 # build the binary, install it, install the plugin
#   omarchy/install.sh --no-build      # skip the cargo build (binary already installed)
#   omarchy/install.sh --bind          # also add the SUPER + SHIFT + R keybinding
#
# Run it on the Omarchy machine, from a checkout of this repo.
set -euo pipefail

plugin_id="mmb.rainnowcast"
plugin_dir="$HOME/.config/omarchy/plugins/$plugin_id"
repo="$(cd "$(dirname "$0")/.." && pwd)"
target="$HOME/.local/bin/rustybui"

build=1
bind=0
for arg in "$@"; do
  case $arg in
    --no-build) build=0 ;;
    --bind) bind=1 ;;
    -h | --help)
      sed -n '2,9p' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

# 1. The binary. The plugin shells out to `rustybui`, which owns fetching, caching
#    and rate limiting; nothing else may talk to the providers.
if ((build)); then
  echo "== building rustybui"
  (cd "$repo" && cargo build --release -p rustybui-cli)
  install -Dm755 "$repo/target/release/rustybui" "$target"
  echo "   installed $target"
fi

if ! command -v rustybui >/dev/null && [[ ! -x $target ]]; then
  echo "rustybui is not on PATH and $target does not exist — rerun without --no-build" >&2
  exit 1
fi

# 2. The plugin itself: a folder of QML that the shell loads in-process.
echo "== installing the plugin into $plugin_dir"
mkdir -p "$plugin_dir"
for file in manifest.json BarWidget.qml Panel.qml NowcastChart.qml Model.js; do
  install -Dm644 "$repo/omarchy/$file" "$plugin_dir/$file"
done

# 3. Tell the running shell to look again, then enable the widget.
if command -v omarchy-shell >/dev/null; then
  omarchy-shell shell rescanPlugins || true
  if command -v omarchy >/dev/null; then
    omarchy plugin enable "$plugin_id" --yes >/dev/null 2>&1 || true
  fi
  echo "   placed on the right-hand side of the bar; move it with: omarchy bar move $plugin_id --section right"
fi

# 4. The keybinding. Only ever touches the user's own bindings file, and only with
#    --bind; the line is idempotent.
bindings="$HOME/.config/hypr/bindings.lua"
if ((bind)); then
  if [[ ! -f $bindings ]]; then
    echo "no $bindings — is Hyprland configured for this user?" >&2
    exit 1
  fi
  if grep -q "$plugin_id" "$bindings"; then
    echo "== keybinding already present in $bindings"
  else
    printf '\n-- Rain nowcast popout (rusty-bui)\no.bind("SUPER + SHIFT + R", "Rain nowcast", { panel = "%s" })\n' "$plugin_id" >>"$bindings"
    echo "== added SUPER + SHIFT + R to $bindings"
    echo "   reload with: hyprctl reload"
  fi
else
  echo
  echo "Add the popout to $bindings with:"
  printf '  o.bind("SUPER + SHIFT + R", "Rain nowcast", { panel = "%s" })\n' "$plugin_id"
  echo "or rerun this script with --bind."
fi

echo
echo "Set the location the pill reports (defaults to whatever is stored):"
echo "  rustybui set-location \"Leiden\" --lat 52.16 --lon 4.49 --country NL --timezone Europe/Amsterdam"
