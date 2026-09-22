#!/bin/zsh
# Wait for the original run, execute the predeclared independent seed pair with
# archived physics, then run development checks. No finalist simulations here.
set -eu
cd /Users/elliot/physics-simulator
export PATH=/Users/elliot/.cargo/bin:$PATH
root=examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19
comparison_pid=${1:?Pass the confirmed live comparison PID}
if ! ps -p "$comparison_pid" -o command= | rg -q 'examples/compare_gait_search .*gait-search-comparison-2026-09-19/comparison-config.json'; then
  print -u2 'Expected live comparison process was not found; no checks started.'
  exit 2
fi
print "Waiting for confirmed comparison PID $comparison_pid before the independent seed extension."
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
console.log('Original trial receipts complete; starting the independent paired seed study.');
JS

RAYON_NUM_THREADS=1 "$root/comparison-host.bin" "$root/extension-config.json" \
  "$root/comparison-independent-seed" 66 "$root/CANCEL-extension" \
  > /tmp/gait-comparison-extension-result.log 2> /tmp/gait-comparison-extension-progress.log
node <<'JS'
const fs=require('fs');
const root='examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/comparison-independent-seed';
for(const [directory,algorithm] of [['Bayesian','bayesian'],['CmaEs','cma_es']])for(let attempt=0;attempt<33;attempt++){
  const path=`${root}/1002301-${directory}-${String(attempt).padStart(3,'0')}/trial.json`;
  const t=JSON.parse(fs.readFileSync(path));
  if(t.seed!==1002301||t.algorithm!==algorithm||t.attempt!==attempt)throw Error(`Mismatched extension receipt: ${path}`);
}
console.log('Independent paired study complete; starting deferred tests.');
JS
CARGO_BUILD_JOBS=2 cargo test -p sim-runtime --features evolution --lib \
  search_comparison::seed_tests > "$root/seed-derivation-tests.log" 2>&1
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
target/release/examples/report_gait_search "$root/comparison-independent-seed" "$root/independent-seed-report"
tar -czf "$root/validation-library-source.tar.gz" Cargo.toml Cargo.lock crates
print 'Both comparison reports generated. Original adjacent seeds overlap; retain the diagnostic run and all costs. Finalist simulations remain pending.'
