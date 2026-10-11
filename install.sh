#!/usr/bin/env bash
# Install rusty-bui on an Omarchy machine: the binary, the bar pill and the
# weather window, plus a launcher entry.
#
#   ./install.sh                # build, install, enable
#   ./install.sh --no-build     # skip cargo (binary already installed)
#   ./install.sh --bind         # also add the keybindings to ~/.config/hypr/bindings.lua
#
# Run from a checkout of this repo, on the machine running Omarchy.
set -euo pipefail

repo="$(cd "$(dirname "$0")" && pwd)"
bin="$HOME/.local/bin/rustybui"
plugins="$HOME/.config/omarchy/plugins"
desktop="$HOME/.local/share/applications/mmb-weather.desktop"
bindings="$HOME/.config/hypr/bindings.lua"

build=1
bind=0
for arg in "$@"; do
  case $arg in
    --no-build) build=0 ;;
    --bind) bind=1 ;;
    -h | --help)
      sed -n '2,10p' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

# ── 1. the binary ────────────────────────────────────────────────────────────
# Both plugins shell out to `rustybui`, which owns fetching, caching and rate
# limiting. Nothing else talks to the providers.
if ((build)); then
  echo "== building rustybui"
  (cd "$repo" && cargo build --release -p rustybui-cli)
  install -Dm755 "$repo/target/release/rustybui" "$bin"
  echo "   installed $bin"
fi

if ! command -v rustybui >/dev/null && [[ ! -x $bin ]]; then
  echo "rustybui is not on PATH and $bin does not exist — rerun without --no-build" >&2
  exit 1
fi

# ── 2. the plugins ───────────────────────────────────────────────────────────
# Staged first, so the plugins that get installed contain no symlinks and each is
# self-contained, then validated with Omarchy's own checker when it is available.
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
"$repo/scripts/stage-plugins.sh" "$stage" >/dev/null

if command -v omarchy-plugin-validate >/dev/null; then
  for id in mmb.rainnowcast mmb.weather; do
    omarchy-plugin-validate "$stage/$id" && echo "== $id: valid"
  done
fi

echo "== installing plugins into $plugins"
for id in mmb.rainnowcast mmb.weather; do
  install -d "$plugins/$id"
  # Drop files a previous version left at the top level, then copy the staged tree.
  find "$plugins/$id" -maxdepth 1 -type f -delete
  for file in "$stage/$id"/*; do
    install -Dm644 "$file" "$plugins/$id/$(basename "$file")"
  done
done

if command -v omarchy-shell >/dev/null; then
  omarchy-shell shell rescanPlugins || true
  if command -v omarchy >/dev/null; then
    for id in mmb.rainnowcast mmb.weather; do
      omarchy plugin enable "$id" --yes >/dev/null 2>&1 || true
    done
  fi
fi

# ── 3. launcher entry for the window ──────────────────────────────────────────
echo "== adding $desktop"
install -d "$(dirname "$desktop")"
cat >"$desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Weather
GenericName=Weather
Comment=Rain nowcast and forecast (KNMI, DWD, Open-Meteo)
Exec=omarchy-shell -q shell toggle mmb.weather '{}'
Icon=weather-few-clouds
Terminal=false
Categories=Utility;Environment;
Keywords=weather;rain;forecast;nowcast;
DESKTOP

if command -v update-desktop-database >/dev/null; then
  update-desktop-database "$(dirname "$desktop")" >/dev/null 2>&1 || true
fi

# ── 4. keybindings ───────────────────────────────────────────────────────────
pill_bind='o.bind("SUPER + SHIFT + R", "Rain nowcast", { panel = "mmb.rainnowcast" })'
window_bind='o.bind("SUPER + ALT + W", "Weather window", "omarchy-shell -q shell toggle mmb.weather '"'"'{}'"'"'")'

if ((bind)); then
  if [[ ! -f $bindings ]]; then
    echo "no $bindings — is Hyprland configured for this user?" >&2
    exit 1
  fi
  if grep -q "mmb.rainnowcast" "$bindings" && grep -q "mmb.weather" "$bindings"; then
    echo "== keybindings already present in $bindings"
  else
    {
      printf '\n-- rusty-bui: rain nowcast popout and weather window\n'
      grep -q "mmb.rainnowcast" "$bindings" || printf '%s\n' "$pill_bind"
      grep -q "mmb.weather" "$bindings" || printf '%s\n' "$window_bind"
    } >>"$bindings"
    echo "== added the keybindings to $bindings (reload with: hyprctl reload)"
  fi
else
  echo
  echo "Add these to $bindings (or rerun with --bind):"
  printf '  %s\n' "$pill_bind"
  printf '  %s\n' "$window_bind"
fi

echo
echo "Set the location both use (until you do, they use the stored one):"
echo "  rustybui set-location \"Leiden\" --lat 52.16 --lon 4.49 --country NL --timezone Europe/Amsterdam"
echo
echo "Checks:  rustybui which · rustybui nowcast · rustybui app --text"
