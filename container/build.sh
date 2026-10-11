#!/usr/bin/env bash
# Build the Arch-based dev/test image (see Containerfile).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
exec podman build -t rustybui-dev:latest -f "$here/Containerfile" "$here"
