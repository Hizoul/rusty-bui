#!/usr/bin/env bash
# Run a command inside the dev image with the repo mounted at /work.
#
#   container/run.sh cargo test
#   container/run.sh cargo test -- --ignored        # live API smoke tests
#   container/run.sh cargo run -p rustybui-cli -- nowcast --lat 52.16 --lon 4.49
#
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
exec podman run --rm -i \
  --userns=keep-id \
  -v "$root":/work:z \
  -v rustybui-cache:/cache \
  -w /work \
  rustybui-dev:latest "$@"
