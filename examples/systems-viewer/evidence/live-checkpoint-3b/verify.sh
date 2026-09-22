#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../../../.."
task_evidence=examples/systems-viewer/evidence/live-checkpoint-3b
task_cargo="${CARGO:-${HOME}/.cargo/bin/cargo}"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false
# Separate commands preserve renderer feature sets and bound compilation load.
nice -n 15 "$task_cargo" test --locked -j 2 -p sim-spatial --lib > "$task_evidence/spatial-tests.log" 2>&1
nice -n 15 "$task_cargo" test --locked -j 2 -p sim-viewer > "$task_evidence/viewer-tests.log" 2>&1
nice -n 15 "$task_cargo" test --locked -j 2 -p sim-runtime --test system_session --test system_worker > "$task_evidence/runtime-tests.log" 2>&1
nice -n 15 "$task_cargo" build --locked -j 2 -p sim-runtime --bin sim-system-worker > "$task_evidence/worker-build.log" 2>&1
nice -n 15 "$task_cargo" build --locked -j 2 -p sim-viewer > "$task_evidence/viewer-build.log" 2>&1
nice -n 15 "$task_cargo" build --locked -j 2 -p sim-spatial > "$task_evidence/spatial-build.log" 2>&1
nice -n 15 "$task_cargo" check --locked -j 2 -p sim-inspect --target wasm32-unknown-unknown > "$task_evidence/wasm-check.log" 2>&1
target/debug/sim-spatial --schematic --live examples/systems-viewer/spatial/motor-thermal.live.json --animation examples/systems-viewer/spatial/motor-thermal.animation.json --validate-only > "$task_evidence/validate-cli.log" 2>&1
