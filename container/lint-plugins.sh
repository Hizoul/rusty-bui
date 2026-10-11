#!/usr/bin/env bash
# Runs inside the build container (called by container/check-qml.sh).
#
#   lint-plugins.sh <omarchy-source-tree> <staged-plugin-dir>...
#
# Validates each staged plugin's manifest with Omarchy's own checker, then type-checks its QML
# against the real shell sources. Findings are counted by rule rather than printed wholesale:
# the classes we know about as one line each, anything else in full, because a class nobody has
# seen before is the one worth reading. Set LINT_DETAIL=1 to print every line anyway.
set -euo pipefail

omarchy="${1:?usage: lint-plugins.sh <omarchy-tree> <plugin-dir>...}"
shift
[[ -d $omarchy/shell ]] || {
  echo "not an Omarchy source tree: $omarchy" >&2
  exit 2
}
[[ $# -gt 0 ]] || {
  echo "no plugin directories given" >&2
  exit 2
}

# Arch ships qmllint outside PATH, in the Qt6 libexec directory.
qmllint_bin="$(command -v qmllint || true)"
[[ -n $qmllint_bin ]] || qmllint_bin=/usr/lib/qt6/bin/qmllint
[[ -x $qmllint_bin ]] || {
  echo "qmllint not found" >&2
  exit 2
}

status=0

# ── manifests ────────────────────────────────────────────────────────────────
echo "== manifests (omarchy-plugin-validate)"
if [[ -f $omarchy/bin/omarchy-plugin-validate ]]; then
  for plugin in "$@"; do
    if bash "$omarchy/bin/omarchy-plugin-validate" "$plugin"; then
      echo "   $(basename "$plugin"): valid"
    else
      status=1
    fi
  done
else
  echo "   (no validator in the checkout, skipping)"
fi

# ── import paths ─────────────────────────────────────────────────────────────
# qmllint resolves `import qs.Commons` from a directory named qs/.
qsroot=/tmp/qsroot
mkdir -p "$qsroot"
ln -sfn "$omarchy/shell" "$qsroot/qs"

# Quickshell itself is not installed here, and qs.Ui's KeyboardPanel extends
# Quickshell's PanelWindow. Without a stub for that base type, qmllint reports the
# KeyboardPanel entry point as "missing required property bar / anchorItem" — an
# artifact of the unresolved base rather than a defect (with the stub both warnings
# disappear; Omarchy's own weather panel only escapes them by bailing out of
# analysis earlier). Only the types we touch are stubbed, so nothing real is hidden.
qsstub=/tmp/qsstub
mkdir -p "$qsstub/Quickshell/Io"
cat >"$qsstub/Quickshell/qmldir" <<'QMLDIR'
module Quickshell
PanelWindow 1.0 PanelWindow.qml
FloatingWindow 1.0 FloatingWindow.qml
QMLDIR
for type in PanelWindow FloatingWindow; do
  printf 'import QtQuick\nItem { }\n' >"$qsstub/Quickshell/$type.qml"
done
cat >"$qsstub/Quickshell/Io/qmldir" <<'QMLDIR'
module Quickshell.Io
Process 1.0 Process.qml
StdioCollector 1.0 StdioCollector.qml
ShellIpc 1.0 ShellIpc.qml
QMLDIR
for type in Process StdioCollector ShellIpc; do
  printf 'import QtQuick\nItem { }\n' >"$qsstub/Quickshell/Io/$type.qml"
done

# ── QML ───────────────────────────────────────────────────────────────────────
# qmllint's noise floor in this setup is high, and all of it comes from the harness rather
# than from our code: dynamic QML resolves its outer scope at run time (`unqualified`), the
# shell's own Commons singletons hide their members behind properties qmllint cannot see
# through (`missing-property` for `Style.font.family` and friends), and Quickshell's types are
# not installed (`import`, `unresolved-type`). Those four are counted; everything else is
# printed in full, since a class this script has not met is the one worth a person's eyes.
#
# What still fails the run: a component we reference that did not make it into the plugin
# ("is not a type"), and any real syntax error in the --bare pass below.
tolerated="unqualified missing-property import unresolved-type"
findings=/tmp/qmllint.all
: >"$findings"
for plugin in "$@"; do
  for file in "$plugin"/*.qml; do
    log=/tmp/qmllint.one
    if "$qmllint_bin" -I "$qsroot" -I "$qsstub" "$file" >"$log" 2>&1; then :; else status=1; fi
    if grep -q "is not a type" "$log"; then
      echo "   !! $(basename "$plugin")/$(basename "$file") references a component that is not in the plugin" >&2
      status=1
    fi
    cat "$log" >>"$findings"
  done
done

if [[ ${LINT_DETAIL:-0} != 0 ]]; then
  cat "$findings"
fi

# A finding is a `Warning:`/`Error:` line; the caret lines, `Info:` suggestions and quoted source
# under it belong to it. Classifying line by line counted every caret as a class nobody had seen.
declare -A counts=()
unexpected=/tmp/qmllint.unexpected
: >"$unexpected"
block=""
while IFS= read -r line; do
  if [[ $line == Warning:* || $line == Error:* ]]; then
    block=""
    if [[ $line == *"]" ]]; then
      block="${line##*[}"
      block="${block%%]*}"
    fi
    if [[ -n $block && " $tolerated " == *" $block "* ]]; then
      counts["$block"]=$(( ${counts["$block"]:-0} + 1 ))
      block=tolerated
    else
      block=unexpected
      printf '%s\n' "$line" >>"$unexpected"
    fi
  elif [[ $block == unexpected ]]; then
    printf '%s\n' "$line" >>"$unexpected"
  fi
done <"$findings"

echo "== qmllint findings by rule (LINT_DETAIL=1 for every line)"
total=0
for tag in $tolerated; do
  if [[ -n ${counts["$tag"]:-} ]]; then
    printf '   %-18s %d\n' "$tag" "${counts["$tag"]}"
    total=$(( total + counts["$tag"] ))
  fi
done
echo "   $total finding(s) in the known classes, none of which fail this run"

unexpected_lines="$(wc -l <"$unexpected")"
if [[ $unexpected_lines -gt 0 ]]; then
  echo "   !! $unexpected_lines line(s) in classes this script has never seen:" >&2
  cat "$unexpected" >&2
  echo "   (not a failure by itself — read them, then add the class to 'tolerated' if it is noise)" >&2
fi

# ── syntax ─────────────────────────────────────────────────────────────────────
# `--bare` ignores imports, so it drowns in unresolved types; only its errors are of interest,
# and its exit status is not trusted to distinguish them (`--bare` with warnings included).
echo "== syntax only (--bare): errors only"
bare=/tmp/qmllint.bare
for plugin in "$@"; do
  for file in "$plugin"/*.qml; do
    "$qmllint_bin" --bare "$file" >"$bare" 2>&1 || true
    if grep -qE "^(Error|Warning: .*\[syntax)" "$bare"; then
      echo "   !! $(basename "$plugin")/$(basename "$file")" >&2
      cat "$bare" >&2
      status=1
    fi
  done
done

exit $status
