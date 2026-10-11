#!/usr/bin/env bash
# Runs inside the build container (called by container/check-qml.sh).
#
#   lint-plugins.sh <omarchy-source-tree> <staged-plugin-dir>...
#
# Validates each staged plugin's manifest with Omarchy's own checker, then
# type-checks its QML against the real shell sources.
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
# qmllint's noise floor in this setup is high (dynamic QML: `unqualified`,
# `missing-property`, `import`, `unresolved-type` for Quickshell's own types), so instead
# of chasing a zero, fail on the two shapes that mean our own code is broken: a component
# we reference that did not make it into the plugin, and a missing base type.
log=/tmp/qmllint.log
for plugin in "$@"; do
  for file in "$plugin"/*.qml; do
    echo "== $(basename "$plugin")/$(basename "$file") (against the Omarchy sources)"
    if "$qmllint_bin" -I "$qsroot" -I "$qsstub" "$file" >"$log" 2>&1; then :; else status=1; fi
    cat "$log"
    if grep -q "is not a type" "$log"; then
      echo "   !! $(basename "$file") references a component that is not in the plugin" >&2
      status=1
    fi
  done
done

echo "== syntax only (--bare, ignores unresolved imports)"
for plugin in "$@"; do
  for file in "$plugin"/*.qml; do
    "$qmllint_bin" --bare "$file" || status=1
  done
done

exit $status
