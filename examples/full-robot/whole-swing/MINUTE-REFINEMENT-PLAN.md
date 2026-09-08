# Further minute-long timestep refinement

The combined teacher passes all physical checks at 2.5 and 1.25 ms, but the
60-second paired body trajectory difference is 0.685 mm, above the existing
0.5 mm screen (foot difference 0.700 mm is below 1 mm). Evaluate the exact
same teacher at 1.25 and 0.625 ms with the same 50 Hz inputs, seed 0, world
push, physical model and solver tolerances. Retain every physical gate and
the 1 mm foot / 0.5 mm body difference screen. Do not enable the experimental
secant solver in this timestep study. Numerical agreement is not calibration.

The repeated 1.25 ms case must reproduce all previous physical frames,
confirming that the default-off solver changes have not changed this model.
This finer native reference is separate from browser realtime measurements.
