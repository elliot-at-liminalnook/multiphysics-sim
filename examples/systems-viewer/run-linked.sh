#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
task_cargo="${CARGO:-${HOME}/.cargo/bin/cargo}"
# Separate builds retain each renderer's existing dependency feature set.
for task_package in sim-viewer sim-spatial; do
  nice -n 15 env CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false \
    "$task_cargo" build --locked -j 2 -p "$task_package"
done
task_target="${CARGO_TARGET_DIR:-target}"
exec "$task_target/debug/sim-spatial" --schematic --connections "$@"
