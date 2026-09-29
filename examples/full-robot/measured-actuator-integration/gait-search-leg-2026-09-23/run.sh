#!/bin/sh
# Try faster reduced profiles first; use the first that qualifies against the
# detailed model at the baseline; then run the study. Cancel: create STOP here.
N=examples/full-robot/measured-actuator-integration/gait-search-leg-2026-09-23
B=target/release/examples
cd /Users/elliot/physics-simulator
[ -f $N/baseline.spec.json ] || (cd $N && python3 make-config.py) && $B/prepare_gait_candidate $N/comparison-config.json $N/baseline.spec.json || { [ -f $N/baseline.spec.json ] || exit 1; }
chosen=""
for p in fast-a fast-b fast-c base; do
  [ -f $N/STOP ] && exit 0
  Q=$N/qualification-$p
  if [ -f $Q/qualification.json ] && grep -q '"qualified":true' $Q/qualification.json; then chosen=$p; break; fi
  [ -d $Q ] || $B/reduced_exploration prepare $N/baseline.spec.json $N/profile-$p.json $Q --fresh || continue
  if $B/reduced_exploration qualify $Q $N/STOP; then chosen=$p; break; fi
  echo "profile $p did not qualify; trying the next"
done
[ -n "$chosen" ] || { echo "no profile qualified"; exit 1; }
echo "using profile $chosen"
(cd $N && python3 make-config.py profile-$chosen.json $N/qualification-$chosen)
$B/compare_gait_search $N/comparison-config.json $N/comparison 160 $N/STOP
