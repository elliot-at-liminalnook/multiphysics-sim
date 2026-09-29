#!/bin/sh
# Qualify the fastest reduced profile (task level) against the detailed
# baseline, reusing one detailed reference run; then search 8 streams at once.
# Cancel: create STOP in this directory.
cd /Users/elliot/physics-simulator
N=examples/full-robot/measured-actuator-integration/gait-search-fast-12v5-2026-09-25
B=target/release/examples
if [ ! -f $N/baseline.spec.json ]; then
  (cd $N && python3 make-config.py) && $B/prepare_gait_candidate $N/comparison-config.json $N/baseline.spec.json > /dev/null || exit 1
fi
chosen=""; reference=""
for p in q32 q16 q8; do
  [ -f $N/STOP ] && exit 0
  Q=$N/qualification-$p
  if [ -f $Q/qualification.json ] && grep -q '"qualified":true' $Q/qualification.json; then chosen=$p; break; fi
  [ -d $Q ] || $B/reduced_exploration prepare $N/baseline.spec.json $N/profile-$p.json $Q --fresh || continue
  [ -n "$reference" ] && [ ! -f $Q/detailed.capture.json ] && cp $reference $Q/detailed.capture.json
  if $B/reduced_exploration qualify $Q $N/STOP; then chosen=$p; break; fi
  [ -f $Q/detailed.capture.json ] && reference=$Q/detailed.capture.json
  echo "profile $p did not qualify; trying the next"
done
[ -n "$chosen" ] || { echo "no profile qualified"; exit 1; }
echo "using profile $chosen"
(cd $N && python3 make-config.py profile-$chosen.json $N/qualification-$chosen)
$B/compare_gait_search $N/comparison-config.json $N/comparison 480 $N/STOP
