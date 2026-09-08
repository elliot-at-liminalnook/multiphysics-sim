# Align the rear support shift with the faster cycle

The all-foot stance adjustment plus derived front support clears the first three
transfers, but the rear thigh/pulley geometry fails during the next support shift.
The body has advanced three transfer intervals by then. Preserve the earlier
rear-lift body position by subtracting `3T(v - 0.0025)` from its forward support
shift, as well as retaining the already declared front and stance adjustments.

For T = 1.38 s the rear shift becomes 3.825 mm at 3.75 mm/s and -1.35 mm at
5 mm/s. All first-cycle foot landing endpoints and both front/rear body support
endpoints now match the earlier geometric baseline, while the intermediate
trajectory and stride cadence differ. This is a derived feasibility hypothesis;
it does not guarantee dynamic support or later-cycle geometry.

Run both commands at 20 and 5 ms physics with the same 24-second task, selected
network, whole-swing horizontal path and half body overlap. The complete physical
and numerical gates in PLAN.md remain unchanged. Retain all four outcomes.
