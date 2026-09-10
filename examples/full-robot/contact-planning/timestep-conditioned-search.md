# Timestep-conditioned trajectory search

Several controller rankings changed when physics was refined from 0.3125 ms
to 0.15625 ms. Matched 300 s tests changed candidate002's net speed by −2.74%,
candidate010's by −5.56%, and the composite negative-steering controller's by
+2.02%. Scalar speed agreement also does not establish trajectory convergence.

The new experiment makes physics timestep an explicit tenth response-model
input alongside the nine controller parameters. Its initial data contain 27
coarser-resolution and four finest-resolution observations. Every label is a
measured first-20-second trajectory response, with shared Rust fits over 5–20 s
and 10–20 s. Full-episode measured speed is reserved for physical qualification;
it is never substituted for a response-model label.

Each finest example's complete scene and command sequence was reconstructed
from the declared controller coordinates. CAD, world, seed and controller
remain matched; only timestep and associated step/report/event counts change.
Contact-enabled and heading-feedback experiments are separate contexts.

Shared Rust `conditioned_design` stratifies free coordinates while fixing an
explicit physical coordinate. Composite EI retains all completed rows for GP
training, but its improvement baseline includes only rows matching the declared
incumbent conditions. Here both the candidate pool and incumbent use 0.15625 ms.
Thus a favorable coarser result cannot silently become the finest-resolution
performance threshold. Empty conditions preserve the previous complete selector
report exactly, including candidate coordinates, predictions and ranking.

The adaptive experiment evaluates one selected physical prefix, appends its
measured responses, and refits before the next selection. Each iteration uses
1,024 local and 1,024 global candidates in the declared controller domain. These
are numerical proposal domains, not CAD limits or a physical speed ceiling.
The timestep coordinate is a simulation context input, not an actuator command.
The controller still receives exactly its nine authored parameters.

This is an ordinary joint-input GP with explicit timestep conditioning. It is
not a reproduction of a cost-aware multi-fidelity optimizer, a calibrated
uncertainty model, or proof of sample-efficiency improvement. The six-point
coarser-only uncertainty scales are not transferred into this new context.
The planar forecasts estimate turning and displacement, not whole-body contact
dynamics or future falls. The separate action-conditioned neural forecaster
remains the component for predicting detailed joint/body motion.

Validation: 50 solver library tests pass, including free-coordinate sampling,
binding rejection, conditioned incumbent selection, and retention of auxiliary
training rows. The complete legacy selector report is unchanged. The first
2,048-candidate proposal contains only the requested finest timestep and retains
all 31 training rows. Input checks and immutable experiment inputs are archived
in `composite-fidelity-evidence-v1.json`; machine status is recorded in
`composite-fidelity-result-v1.json`. New speed gains require completed physical
evaluation, followed by sustained qualification. Physical maximum remains unproved.
