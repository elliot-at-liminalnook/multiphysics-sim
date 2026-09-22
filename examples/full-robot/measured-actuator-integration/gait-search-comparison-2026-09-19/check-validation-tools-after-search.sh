#!/bin/zsh
# Deferred development checks only. Never launches or resumes optimizer trials.
set -eu
cd /Users/elliot/physics-simulator
export PATH=/Users/elliot/.cargo/bin:$PATH
root=examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19
comparison_pid=${1:?Pass the confirmed live comparison PID}
if ! ps -p "$comparison_pid" -o command= | rg -q 'examples/compare_gait_search .*gait-search-comparison-2026-09-19/comparison-config.json'; then
  print -u2 'Expected live comparison process was not found; no checks started.'
  exit 2
fi
print "Waiting for confirmed comparison PID $comparison_pid before build/test work."
while kill -0 "$comparison_pid" 2>/dev/null; do
  sleep 30
done

# A stopped process is not proof that the study completed.
node <<'JS'
const fs=require('fs');
const root='examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/comparison';
const c=JSON.parse(fs.readFileSync(`${root}/config.json`));
for(const seed of c.optimizer_seeds)for(const [directory,algorithm] of [['Bayesian','bayesian'],['CmaEs','cma_es']]){
  for(let attempt=0;attempt<c.attempts_per_algorithm_seed;attempt++){
    const path=`${root}/${seed}-${directory}-${String(attempt).padStart(3,'0')}/trial.json`;
    const trial=JSON.parse(fs.readFileSync(path));
    if(trial.seed!==seed||trial.algorithm!==algorithm||trial.attempt!==attempt)throw Error(`Mismatched trial: ${path}`);
  }
}
console.log('Every declared trial receipt exists; starting deferred helper checks.');
JS

CARGO_BUILD_JOBS=2 cargo test -p sim-runtime --features evolution \
  --test experiment_variants --test geometry_evaluation --test fidelity \
  --test search_comparison > "$root/validation-helper-tests.log" 2>&1
CARGO_BUILD_JOBS=2 cargo build --release -p sim-runtime --features evolution \
  --example prepare_validation_case --example prepare_timestep_case \
  --example report_gait_search --example evaluate_validation_capture \
  --example evaluate_timestep_captures \
  --example compare_environment_fidelity > "$root/validation-helper-build.log" 2>&1
print 'Validation helper tests and builds passed.'
target/release/examples/report_gait_search "$root/comparison" "$root/search-report"
print 'Comparison report generated. Finalist simulations remain pending.'
