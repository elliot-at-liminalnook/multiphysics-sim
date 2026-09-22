#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
task_cargo="${CARGO:-${HOME}/.cargo/bin/cargo}"
nice -n 15 env CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false \
  "$task_cargo" build --locked -j 2 -p sim-runtime -p sim-viewer --bin sim-system-worker --bin sim-viewer
exec examples/systems-viewer/run-linked.sh --live examples/systems-viewer/spatial/motor-thermal.live.json --animation examples/systems-viewer/spatial/motor-thermal.animation.json "$@"
