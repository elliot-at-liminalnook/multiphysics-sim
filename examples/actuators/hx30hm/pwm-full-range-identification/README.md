# Full-range short-pulse identification

Shared Rust empirical fits from the completed 216-trial individual 150 ms sweep in ../hardware/2026-09-11-nine-servos/pwm-individual-full-range. This predates the full-rotation experiments.

81/162 held-out trials meet both predeclared criteria: encoder RMSE <=3 counts and final displacement error <=5 counts. The model is therefore insufficient across the full range. Optimizer convergence is not validation. Results retain fitted parameters, predictions, input hashes, bounds and timing sensitivity. No fitted value was promoted into CAD.

Training uses individual positive PWM 25/100/300/500/700/900; other positive levels and all reverse pulses are held out. An empirical integrated first-order response does not separately identify motor resistance, inductance, torque constant, inertia, friction or load torque. Later full-rotation and torque-off measurements require separate validation and distinguish braking from coasting.
