# Rear-foot placement and startup order

With whole-swing motion and both derived support shifts, 3.75 mm/s reaches step
6 but rejects the rear-leg geometry during body return. The rear foot was moved
back along with the front foot; this worsens its extension as the body advances.
At 5 mm/s the rear leg reaches its limit before its first transfer.

The next eight declared cases use 3.75/5 mm/s, each at 20/5 ms physics, and two
orders: the original [0,2,1,3] and rear-first [3,0,2,1]. Shift lateral/front
stance targets back by `4T(v-v0)` as before, but shift the rear stance target
forward by that amount. This shortens rear-leg extension while retaining the
front-leg forward workspace. The offsets are policy parameters; CAD geometry
and physical properties stay unchanged.

For each foot, set its X support offset to the baseline offset plus
`T * (baseline_slot * v0 - new_slot * v)`, where v0 = 2.5 mm/s and T = 1.38 s.
Thus its first body support endpoint matches the earlier geometric baseline
despite the changed speed/order. Other planted feet may differ, so this is a
feasibility hypothesis, not a balance certificate. Half body overlap, whole-swing
horizontal motion, neural weights and all PLAN.md acceptance gates remain fixed.
Keep every incomplete and rejected result. Forward walking only is tested here.
