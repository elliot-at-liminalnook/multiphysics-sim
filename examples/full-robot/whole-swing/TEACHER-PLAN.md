# Diagnose tracking with the existing privileged teacher

The 85% finish profile retains 13 qualified swings at both timesteps, but the
5 ms endpoint fails and maximum foot/body disagreement remains above budget.
Before searching student weights, replace its learned motor correction with
the existing privileged body/foot feedback teacher. This isolates whether
feedback based on actual body and foot positions improves the new trajectory.

Evaluate exactly two 24-second cases, at 20 and 5 ms physics steps, retaining
the asymmetric 3.75 mm/s recipe and all action inputs. Restore the existing
teacher's shared body/point correction observations; zero the student
network output to avoid adding both estimates of the same correction. Keep its
original 0.5 joint gain, 0.25 body/point gains and support-dependent standing
increment. No motor, geometry, task, contact or acceptance changes are allowed.

This is corrective development, not promotion of the failed student. Report
all physical gates and paired trajectory screens from PLAN.md. If successful,
collect teacher trajectories for a new student; ideal world pose and contact
observations must remain explicitly privileged and unavailable as purported
hardware sensor measurements. Browser runtime cost is assessed separately.

The first initialization attempt removed the network entirely. The unchanged
task rejected its missing neural-correction observation channels before any
physics ran; `teacher-initialization-failure.json` preserves that failure.
Zeroing the final layer retains those channels and their exact zero values,
allowing the same task definition and rewards to be used.
