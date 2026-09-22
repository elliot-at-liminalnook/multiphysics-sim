# Rejected timing demonstration

`two-motor-source-timing-rejected.json` and its PNG/SVG plots retain the initial
synthetic source-accounting run. Its fixture used `t < 0.01` at controller ticks;
a floating-point tick just below 10 ms kept PWM active until 11 ms. The accounting
for that actual trace was consistent, but the nominal 10 ms pulse caption and
intended controller schedule were not. These files are not the accepted timing
case and must not supply its numerical result.

The initial test source is retained in
`verification/two-motor-source-initial-fixture.rs.txt`. The corrected fixture uses
a numerical tolerance at the declared time boundary and explicitly checks that
the controller commands zero at 10 ms and source draw stops within one retained
0.1 ms sample. An intermediate test incorrectly demanded a zero-current sample
exactly on a pre-event floating-point endpoint; that failed log is also retained. `two-motor-source-report.json` and `two-motor-source`
plots contain the corrected result. No hardware action was involved.
