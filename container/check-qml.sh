#!/usr/bin/env bash
# Check the Omarchy plugin folder: manifest against Omarchy's own validator, QML
# against the real shell sources.
#
#   container/check-qml.sh /path/to/omarchy-checkout
#
# The checkout only needs its shell/ directory (imports `qs.Commons`, `qs.Ui`) and
# bin/ (the manifest validator). Quickshell itself is not installed here, so
# `Quickshell`/`Quickshell.Io` imports stay unresolved — that is expected and does
# not stop the checks that matter: our use of the qs.* singletons and base types.
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
    set -u
    status=0

    # Arch ships qmllint outside PATH, in the Qt6 libexec directory.
    qmllint_bin="$(command -v qmllint || true)"
    [[ -n $qmllint_bin ]] || qmllint_bin=/usr/lib/qt6/bin/qmllint
    [[ -x $qmllint_bin ]] || { echo "qmllint not found" >&2; exit 2; }

    echo "== manifest (omarchy-plugin-validate)"
    if [[ -f /omarchy-src/bin/omarchy-plugin-validate ]]; then
      if bash /omarchy-src/bin/omarchy-plugin-validate /work/omarchy; then
        echo "   valid"
      else
        status=1
      fi
    else
      echo "   (no validator in the checkout, skipping)"
    fi

    # qmllint resolves `import qs.Commons` from a directory named qs/.
    mkdir -p /tmp/qsroot
    ln -sfn /omarchy-src/shell /tmp/qsroot/qs

    for file in /work/omarchy/*.qml; do
      echo "== ${file#/work/} (against the Omarchy sources)"
      "$qmllint_bin" -I /tmp/qsroot "$file" || status=1
    done

    echo "== syntax only (--bare, ignores unresolved imports)"
    for file in /work/omarchy/*.qml; do
      echo "-- ${file#/work/}"
      "$qmllint_bin" --bare "$file" || status=1
    done

    exit $status
  '
