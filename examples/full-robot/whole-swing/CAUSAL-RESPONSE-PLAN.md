# Command-to-body-response development measurement

Use the existing direct-transfer 24-second steering recipe, with optional foot
damping absent. Freeze a repeated commanded run plus four counterfactual runs.
Each counterfactual holds the preceding input for exactly one command interval:
forward at 0 s (hold zero), turn at 8.4 s (hold forward), reverse at 16.8 s (hold
turn), and stop at 20 s (hold reverse). Restore the original subsequent inputs.
Keep scene, configuration, task, seed and physical parameters identical. Verify
every physical frame through the branch time exactly, and verify that only the
declared input interval differs. Retain task failures; they cannot be treated as
controller qualification. Repeated commanded frames must match the prior
accepted steering run exactly, excluding frame wall time.

Before evaluating the counterfactuals, declare **0.1 mm** directed body-position
difference for forward, reverse and stop, or **0.0005 rad** yaw difference for
turn. These are 10% of the existing 1 mm position and 0.005 rad heading acceptance
budgets, well above the prior native/WASM difference. Require consecutive 50 Hz
samples above threshold for **0.1 simulated seconds**. These are sensitivity
definitions, not new acceptance gates or hardware sensor-resolution claims.

Project translation on the actual body's forward axis at the branch time.
Forward uses positive displacement, reverse negative; stop uses positive
displacement relative to continued reverse. Turn uses positive yaw difference.
Also retain absolute horizontal and yaw differences so wrong-way motion is
visible. Search only until the next original command (or 24 s for stop). A
missing sustained crossing stays missing. No interpolation or between-sample
guarantee is made. Detection latency is not settling time or completed reversal.

Associate the commanded native trajectory with the already measured browser
recording, which has the same recipe, seed and input sequence. Map the first
sustained physical threshold crossing to its worker receipt and first matching
drawn frame using the recorded single-page clock. This is a native paired-model
measurement associated with browser timestamps, not a direct counterfactual
WASM experiment, monitor presentation measurement, hardware latency or an
accuracy certificate for the direct gait. Preserve that distinction in results.
