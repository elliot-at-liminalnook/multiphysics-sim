#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
task_cargo="${CARGO:-${HOME}/.cargo/bin/cargo}"
# The same lean development profile used for the verified Intel Mac preview.
# Lower build priority/parallelism so the user's desktop remains responsive.
nice -n 15 env CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false \
  "$task_cargo" build --locked -j 2 -p sim-spatial
# Run the GUI at normal priority after the low-priority build exits.
task_target="${CARGO_TARGET_DIR:-target}"
exec "$task_target/debug/sim-spatial" "$@"
