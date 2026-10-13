#!/usr/bin/env bash
# Install rusty-bui on an Omarchy machine: the binary, the bar pill and the
# weather window, plus a launcher entry.
#
#   ./install.sh                # build, install, enable
#   ./install.sh --dry-run      # say what it would do, write nothing
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
ids=(mmb.rainnowcast mmb.weather)

build=1
bind=0
dry=0
for arg in "$@"; do
  case $arg in
    --no-build) build=0 ;;
    --bind) bind=1 ;;
    --dry-run) dry=1 ; build=0 ;;
    -h | --help)
      sed -n '2,11p' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

if ((dry)); then
  echo "== dry run: nothing below is written, staged or started"
fi

# ── 1. the binary ────────────────────────────────────────────────────────────
# Both plugins shell out to `rustybui`, which owns fetching, caching and rate
# limiting. Nothing else talks to the providers.
#
# Where cargo puts the binary is not always <repo>/target: CARGO_TARGET_DIR and a
# `build.target-dir` in ~/.cargo/config.toml both move it, and asking for a path
# that is not there is what turned a finished build into "install: cannot stat".
# So the build runs first, then the path is asked for out of cargo metadata, which
# is the one place that knows.
if ((build)); then
  echo "== building rustybui"
  (cd "$repo" && cargo build --release -p rustybui-cli)
fi

if ((dry)); then
  echo "   would install the rustybui binary to $bin"
  if command -v cargo >/dev/null; then
    echo "   target directory: $(cargo metadata --format-version 1 --no-deps --manifest-path "$repo/Cargo.toml" 2>/dev/null |
      grep -o '"target_directory":"[^"]*"' | cut -d'"' -f4)"
  fi
elif ((build)); then
  # `|| true` so a cargo metadata that fails falls through to the plain path and gets the
  # message below, rather than ending the script with no explanation at all under `set -e`.
  target="$(cargo metadata --format-version 1 --no-deps --manifest-path "$repo/Cargo.toml" 2>/dev/null |
    grep -o '"target_directory":"[^"]*"' | cut -d'"' -f4 || true)"
  binary="${target:-$repo/target}/release/rustybui"
  if [[ ! -x $binary ]]; then
    echo "cargo finished but $binary is not there." >&2
    echo "If the build ran somewhere else (a container, or with CARGO_TARGET_DIR set), run this" >&2
    echo "installer from the same shell — or check: cargo metadata --format-version 1 --no-deps | grep target_directory" >&2
    exit 1
  fi
  install -Dm755 "$binary" "$bin"
  echo "   installed $bin (from $binary)"
fi

if ! command -v rustybui >/dev/null && [[ ! -x $bin ]]; then
  if ((dry)); then
    # A dry run is asked about the machine as it is, not as it will be after the build:
    # note the absence and carry on to the part worth rehearsing.
    echo "   note: rustybui is not on PATH and $bin does not exist yet"
  else
    echo "rustybui is not on PATH and $bin does not exist — rerun without --no-build" >&2
    exit 1
  fi
fi

# ── 2. the plugins ───────────────────────────────────────────────────────────
# Staged first, so the plugins that get installed contain no symlinks and each is
# self-contained, then validated with Omarchy's own checker when it is available.
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
"$repo/scripts/stage-plugins.sh" "$stage" >/dev/null

if command -v omarchy-plugin-validate >/dev/null; then
  for id in "${ids[@]}"; do
    omarchy-plugin-validate "$stage/$id" && echo "== $id: valid"
  done
fi

if ((dry)); then
  # The question a dry run is actually asked: what would change? A plugin the
  # installer has run before holds the previous round's files, and the only way to
  # see that the new ones land (and the dropped ones go) is to compare the trees.
  for id in "${ids[@]}"; do
    echo "== would install $id into $plugins/$id"
    if [[ -d $plugins/$id ]]; then
      # diff exits 0 when the trees match, which is the only way to say "nothing to do" —
      # piping it into sed would lose that status.
      if changes="$(diff -rq "$plugins/$id" "$stage/$id" 2>/dev/null)"; then
        echo "   (identical — already installed)"
      else
        printf '%s\n' "$changes" | sed \
          -e "s|^Files $plugins/$id/\(.*\) and .* differ$|   updated  \1|" \
          -e "s|^Only in $stage/$id: \(.*\)$|   added    \1|" \
          -e "s|^Only in $plugins/$id: \(.*\)$|   removed  \1|"
      fi
    else
      echo "   new plugin — $(find "$stage/$id" -type f | wc -l) files"
      for file in "$stage/$id"/*; do echo "   added    $(basename "$file")"; done
    fi
  done
else
  echo "== installing plugins into $plugins"
  for id in "${ids[@]}"; do
    install -d "$plugins/$id"
    # Drop files a previous version left at the top level, then copy the staged tree.
    find "$plugins/$id" -maxdepth 1 -type f -delete
    for file in "$stage/$id"/*; do
      install -Dm644 "$file" "$plugins/$id/$(basename "$file")"
    done
  done
fi

if ((!dry)) && command -v omarchy-shell >/dev/null; then
  omarchy-shell shell rescanPlugins || true
  if command -v omarchy >/dev/null; then
    for id in "${ids[@]}"; do
      omarchy plugin enable "$id" --yes >/dev/null 2>&1 || true
    done
  fi
fi

# ── 3. launcher entry for the window ──────────────────────────────────────────
if ((dry)); then
  echo "== launcher entry:"
  echo "   would write $desktop"
else
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
fi

# ── 4. keybindings ───────────────────────────────────────────────────────────
# Both are plain shell commands, the way Omarchy binds its own weather toggle
# (`o.bind("SUPER + CTRL + ALT + W", "Toggle weather", "omarchy-notification-weather")`):
# `shell toggle <id>` is the documented IPC — "summon if closed, hide if open" — and it
# works for a bar widget's popout, which is what the pill's detail panel is. The
# `{ panel = "<id>" }` shorthand is for panel plugins and is left as a comment below.
pill_bind='o.bind("SUPER + SHIFT + R", "Rain nowcast", "omarchy-shell -q shell toggle mmb.rainnowcast")'
window_bind='o.bind("SUPER + ALT + W", "Weather window", "omarchy-shell -q shell toggle mmb.weather '"'"'{}'"'"'")'

if ((dry)); then
  echo "== would add these keybindings to $bindings (with --bind):"
  printf '   %s\n' "$pill_bind"
  printf '   %s\n' "$window_bind"
elif ((bind)); then
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
  echo "  (as a panel plugin bind, if you prefer: { panel = \"mmb.rainnowcast\" })"
fi

# ── 5. window rule ────────────────────────────────────────────────────────────
# The weather window is a Quickshell window — class `org.quickshell`, title `Weather`.
# Tiled, it opens into the workspace beside whatever is already there; it is an overlay,
# so it should float and open at the top, clear of the bar. Hyprland decides that, not the
# plugin, so the rule goes in the user's own config. A previous rusty-bui weather rule is
# replaced, so changing the placement later updates it instead of being skipped.
rules="$HOME/.config/hypr/hyprland.lua"
rule_comment='-- rusty-bui: the weather window floats and opens at the top, below the bar'
window_rule='o.window({ class = "^org.quickshell$", title = "^Weather$" }, { float = true, move = { "(monitor_w-window_w)/2", "34" } })'

if ((dry)); then
  echo
  echo "== would write this into $rules:"
  printf '   %s\n   %s\n' "$rule_comment" "$window_rule"
elif [[ -f $rules ]]; then
  if grep -qF "$window_rule" "$rules"; then
    echo "== weather window rule already present in $rules"
  elif grep -qF 'title = "^Weather$"' "$rules"; then
    tmp="$(mktemp)"
    awk -v rule="$window_rule" -v comment="$rule_comment" '
      index($0, "title = \"^Weather$\"") { print rule; next }
      index($0, "-- rusty-bui: the weather window") { print comment; next }
      { print }
    ' "$rules" >"$tmp"
    mv "$tmp" "$rules"
    echo "== updated the weather window rule in $rules (reload with: hyprctl reload)"
  else
    {
      printf '\n%s\n%s\n' "$rule_comment" "$window_rule"
    } >>"$rules"
    echo "== added the weather window rule to $rules (reload with: hyprctl reload)"
  fi
else
  echo "no $rules — skipping the weather window rule" >&2
fi

if ((dry)); then
  echo
  echo "== dry run done: nothing was installed"
  exit 0
fi

echo
echo "Set the location both use (until you do, they use the stored one):"
echo "  rustybui set-location \"Leiden\" --lat 52.16 --lon 4.49 --country NL --timezone Europe/Amsterdam"
echo
echo "Then:    rustybui which · rustybui app --text · rustybui day · rustybui radar --json"
echo "Settings: rustybui prefs (clock, hours, units, offline) · rustybui places --add"
