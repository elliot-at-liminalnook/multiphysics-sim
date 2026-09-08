# Display cadence and control delivery

The portable tangent profile keeps active simulation pace but misses the
20 ms control-transition gate: steering 20.84 ms, forward 22.24 ms. The same
binary without tangent probes measures 26.26 ms steering. Across active phases,
transport/dispatch p95 is about 7–8 ms; return-phase WASM solves remain costly.

Add an explicit optional 30 fps display cap, retaining automatic display as
the default. Physics/control stay at their existing 50 Hz task contract with
the identical Rust/WASM module, model, controller, inputs and solver settings.
Rendering only draws the latest completed frame. Do not change pacing,
observations, forces, solver tolerances or task gates. Camera drawing follows
the selected display cadence. Keep the display choice visible and record it
in performance reports.

Measure automatic and capped steering plus capped forward/stop sequentially
on the same bundle without concurrent expensive work. Record actual draws,
worker/transport breakdown, command-reference drawing delays and both active
and overall pace. Retain all results; require the original >=1 simulated/wall
second and <=20 ms p95 gates. Verify identical recordings/actions against the
existing physically audited captures. Then run exact-load/replay/video/mobile
UI checks for the new display control. A cap can trade visual cadence for
processing headroom; it cannot qualify coarse physics, robustness or terrain.
