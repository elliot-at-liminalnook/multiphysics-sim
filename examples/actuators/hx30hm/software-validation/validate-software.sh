#!/usr/bin/env bash
# Software-only. No serial port, programmer, or hardware command is used.
set -euo pipefail
hx_repo=$(cd "$(dirname "$0")/../../../.." && pwd)
hx_fpga=${1:?usage: validate-software.sh SIPEED_SERVO_DIRECTORY NEW_OUTPUT_DIRECTORY}
hx_output=${2:?provide a new output directory}
hx_cargo=${HX_CARGO:-cargo}
mkdir "$hx_output"
hx_output=$(cd "$hx_output" && pwd)
hx_fpga=$(cd "$hx_fpga" && pwd)
cd "$hx_repo"
hx_plan=examples/actuators/hx30hm/software-validation
make -C "$hx_fpga" sim-safety > "$hx_output/fpga-simulation.log" 2>&1
make -C "$hx_fpga" bridge-safety > "$hx_output/fpga-build.log" 2>&1
"$hx_cargo" test --release -p sim-runtime --lib acquisition:: > "$hx_output/library-tests.log" 2>&1
"$hx_cargo" test --release -p sim-runtime --example characterize_hx_bridge > "$hx_output/runner-tests.log" 2>&1
"$hx_cargo" test --release -p sim-domain-robot --test motor > "$hx_output/motor-tests.log" 2>&1
"$hx_cargo" run --release -p sim-runtime --example characterize_hx_bridge -- \
  --validate-sweep "$hx_plan/pwm-sweep-plan.json" 4,5,6,7,8,9,10,11,12 > "$hx_output/sweep-schedule.json"
"$hx_cargo" run --release -p sim-runtime --example characterize_actuator -- \
  "$hx_plan/pwm-physics-plan.json" "$hx_output/pwm-physics-results" > "$hx_output/physics.log" 2>&1
"$hx_cargo" run --release -p sim-runtime --example characterize_actuator -- \
  "$hx_plan/pwm-convergence-plan.json" "$hx_output/pwm-convergence-results" > "$hx_output/convergence.log" 2>&1
node "$hx_plan/check-physics.mjs" "$hx_output"
