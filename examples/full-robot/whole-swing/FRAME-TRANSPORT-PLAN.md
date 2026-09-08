# Frame transport experiment

Automatic versus 30 fps drawing changes actual draw count substantially but
does not reduce active steering p95 (20.710 versus 20.715 ms), and changes
transport/dispatch p95 only from 7.36 to 7.30 ms. The current worker parses
Rust's JSON frame into an object, then postMessage clones that nested object
to the viewer. Worker timing ends before outgoing cloning and delivery.

Test an opt-in JSON-text encoding for step replies. Reuse the exact string
returned by Rust, parse it once in the receiver, and retain the existing object
encoding for compatibility and same-bundle comparisons. Change transport only:
same compiled WASM, exact controller/model, 50 Hz task, inputs, solver settings,
automatic display and pacing. Preserve request IDs, errors, progress, reset,
replay, cancellation and invalid-action state preservation. Reject unsupported
encoding choices before stepping. Keep protocol details out of user controls.

Require complete native/WASM physical/task comparison at the existing fixed
tolerance and exact same-host replay/reset before rendered timing. Compare
object and JSON steering and JSON forward/stop sequentially on the same bundle.
Include receiving JSON parse time in measured transition latency. Retain all
outcomes and original >=1 pace / <=20 ms p95 gates. Verify exact recorded
recipes/inputs and UI load/replay/video/mobile behavior. This is a transport
experiment, not a physics approximation, and cannot qualify the existing
coarse timestep, sustained walking, held-out disturbances or terrain.
