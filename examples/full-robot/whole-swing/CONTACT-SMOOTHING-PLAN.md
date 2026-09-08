# Contact smoothing sensitivity for the frozen integral teacher

The 1 mm/s regularized-Coulomb smoothing profile passes the frozen task and
timestep screens but has 152–198 mm accumulated load-weighted tangential
contact motion per foot during 213 mm body advance. This is a material-velocity
diagnostic, beyond geometric foot-marker travel. It exposes a separate physical
quality limit; existing task passes are retained with that limitation.

Before seeing alternative-profile results, freeze two 60-second forward cases
with smoothing speeds **0.1 mm/s** and **0.03 mm/s**, at 1.25 ms backward Euler.
Use the exact integral teacher, CAD artifact, force coefficients, motor limits,
solver tolerances, original command inputs and development push. Only the
explicit regularized-Coulomb smoothing option changes. This parameter is a
numerical constitutive approximation, not a newly measured material property.
Record each as a distinct fidelity profile, with its own task outcome and cost.

Retain every result, including runtime errors and accepted prefixes. Measure
actual sustained travel, body advance, supported swings, stopping, heading,
work and load-weighted contact motion. Do not interpret agreement between two
different smoothing models as a timestep-accuracy test. The retained 1 mm/s
minute supplies the original-profile baseline and remains unchanged.

Predeclare a prospective anti-sliding screen: the largest per-foot integral of
load-weighted tangential contact speed must be no more than **5% of net
horizontal body advance** over the full minute, and the original task gates
must pass. This permits about 1 mm loaded motion per foot per roughly 20 mm
four-transfer body cycle, consistent in scale with the existing 1 mm foot
trajectory budget. It is a provisional simulation-quality screen, not a
calibrated wear, terrain or hardware criterion. Trapezoidal sampled velocities,
changing contact weights and omitted contact birth/death intervals limit it;
passing alone does not certify absence of between-sample sliding.

Do not change this threshold after seeing these results. If a profile passes,
its timestep refinement, other commands and browser cost need separate tests
before promotion. If none passes, retain the demonstrated limit and investigate
contact laws and tracking rather than presenting a higher requested speed as
an improvement. These previously revealed command inputs are development
cases; no fresh held-out claim is made.
