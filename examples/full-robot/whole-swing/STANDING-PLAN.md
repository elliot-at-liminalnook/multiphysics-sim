# Recover the sustained stopping margin

The 5/2.5 ms teacher minutes complete all 41 swings but fail final position at
1.092/1.080 mm. The 5 ms endpoint error components are [+0.689,-0.505,-0.680] mm;
all four final foot forces are 8.54–10.44 N, above the teacher's existing 5 N
full-support threshold. Its standing body gain therefore reaches only 0.75.

Repeat the two unchanged minute recipes with `standing_gain_increment` raised
from 0.5 to 1.25, giving a fully supported standing body gain of 1.5 instead of
0.75. This doubles the existing corrective gain only when motion commands are
zero and all feet support the body. It changes no moving gains, student weights,
actuator parameters, angle bounds, posture, contact law or task budget.

This single paired test checks the proportional-feedback explanation for the
steady endpoint error. It may introduce oscillation or contact failure; retain
either outcome. Require all physical and paired numerical gates from
SUSTAINED-PLAN.md. Do not change the 1 mm endpoint limit for either near miss.
