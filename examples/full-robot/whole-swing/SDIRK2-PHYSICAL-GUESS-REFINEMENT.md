# Refinement after removing the coarse root jump

With first-stage velocity guesses, both 20 ms controllers finish but fail
stopping. Teacher differs from its retained 1.25 ms backward-Euler trajectory
by 1.766 mm foot / 1.669 mm body. Keep these failures unchanged.

Run the same 24-second teacher steering inputs with SDIRK2 at 10 and 5 ms.
Also capture backward Euler at 0.625 ms with the identical teacher, scene,
commands and tolerances. This resolves a reference limitation: the previous
5 ms SDIRK2 versus 1.25 ms backward-Euler body difference was 0.564 mm, close
to the 0.5 mm budget, while the retained backward-Euler minute itself differed
by 0.470 mm between 1.25 and 0.625 ms. A more accurate integrator can differ
from a finite-step reference; that possibility must be measured.

Retain all original task gates and the 1 mm foot / 0.5 mm body difference
budgets. Compare 10/5 ms SDIRK2, both against the 1.25 ms reference, and against
the new 0.625 ms reference. Check 1.25/0.625 ms backward Euler on these exact
steering commands. Report every comparison, including any conflict; do not
erase the earlier reference failures or call the finest run ground truth.
Measure native compute sequentially. This screen does not establish minute,
held-out, terrain, browser or hardware qualification.

The versioned `sdirk-physical-refinement-plan.json` contains exact cases and
source hashes. Regenerate earlier authored recipes with their pinned preparation
scripts, copy this plan to `runs/full-robot/learning/sdirk-physical-refinement/plan.json`,
then execute the shared `examples/full-robot/hybrid-speed/run.mjs` runner.
