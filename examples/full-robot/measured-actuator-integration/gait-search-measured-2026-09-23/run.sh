#!/bin/sh
# Qualify the reduced model at the measured-actuator baseline, then run the study.
# Cancel: create STOP in this directory.
set -e
N=examples/full-robot/measured-actuator-integration/gait-search-measured-2026-09-23
B=target/release/examples
if [ ! -f $N/qualification/qualification.json ]; then
  [ -d $N/qualification ] || $B/reduced_exploration prepare $N/baseline.spec.json $N/profile.json $N/qualification --fresh
  $B/reduced_exploration qualify $N/qualification $N/STOP
fi
$B/compare_gait_search $N/comparison-config.json $N/comparison 160 $N/STOP
