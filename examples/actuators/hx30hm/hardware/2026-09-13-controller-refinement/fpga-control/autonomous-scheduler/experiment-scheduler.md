# Device-timed experiment scheduler — integration in progress

`src/experiment_scheduler.v` is a synthesizable sequencer core. It is **not yet
connected to bridge_control**, and no image containing it has been loaded.
The existing hardware still uses host-scheduled 100/150 ms experiments.

The core freezes a selected motor mask, frame count and period at an explicit
start edge. Default bounds are 40–150 ms at 50 MHz, at most 256 frames and at most
12 seconds total. The 40 ms floor is a provisional target, not a measured hardware
capability or controller-accuracy claim. Trajectories, gains and PWM remain in the
shared-controller/packet adapter; the scheduler contains no second control law.

## Per-frame sequence

1. Wait for the validated, immutable trajectory row for `frame_index`.
2. Request each selected motor's telemetry read, address 0x38, width 15.
   Require its solicited, checksum-valid healthy sample sequence to advance.
3. Request the existing A1 controller operation. The adapter runs the compiled
   integer law and sends the synchronized PWM batch through the independent
   supervisor's ordinary write gating. Await final wire transmission.
4. Read torque enable and PWM at address 0x28, width 6, for every selected motor.
   Require torque enabled and PWM equal to the command actually transmitted.
5. Hold until the next fixed device-clock frame boundary. After the last frame,
   request the independent stop pair and await its final wire transmission.

Missed boundaries abort. There are no catch-up bursts or extensions for missing
rows, blocked arbitration, missing feedback or slow audit replies. Configuration
validity loss, start collisions, cancellation, lost arm bits, supervisor trips and
logging backpressure also stop. A held start cannot restart after termination.
The core never arms motors or renews command leases; host-disconnect protection
remains independent. It does not infer mechanical stationarity from transmitted
stop packets.

## Packet adapter contract

- `request_valid && request_ready` accepts one whole transaction. `request_ready`
  means bus ownership and transport capacity are reserved; only one transaction
  is outstanding. No native host bytes may be interleaved into an autonomous
  packet. Host STOP/heartbeat/status must use buffered packet arbitration.
- Kinds 0/1/2 mean telemetry read / A1 controller / torque-PWM audit. A1 uses the
  frozen trajectory row and gains, not host-computed PWM.
- Only the supervisor's accepted telemetry increments `sample_sequences`.
  Unrelated reads, unsolicited packets, bad CRC/status and old samples cannot
  acknowledge a poll. Audit replies require the expected ID/address/width,
  checksum and status, torque=1, and exact transmitted PWM.
- Trajectory storage is locked during a session. Assert `plan_valid` only after
  whole-plan verification and `row_ready` only when the indexed row is available.
  Revoke validity if the plan is changed or corrupted.
- `control_wire_done` is the final transmitted byte, not queue acceptance.
  `stop_pair_wire_done` is after both spaced stop packets finish, not merely the
  existing supervisor's `stop_accepted` pulse (which starts transmission).

Each completed transaction emits its kind, motor, frame, sequence, outcome and
64-bit request/completion clock times. Events use valid/ready consumption. No next
request is accepted while an event is pending; withdrawn credit stops the run and
holds that event for draining. Failure also preserves whether a transaction was
in flight, its kind/ID/request time, stop-request time and stop-pair completion time.
The event adapter must attach the actual raw telemetry, audit bytes and transmitted
PWM; current scaling remains uncalibrated.

## Verification

```sh
make sim-experiment-scheduler
make check-experiment-scheduler
```

Fifteen cycle-level cases cover all nine motors, sparse masks, deadlines, missing
fresh feedback, evidence overflow/backpressure, wrong or failed audit replies,
supervisor/arm loss, cancellation with in-flight evidence, missing rows, invalid
configuration and held-start/reconfiguration rejection. Gowin synthesis passes
with `-nodsp -nolutram`. These are component tests with a transaction adapter model,
not complete UART tests, place-and-route timing or physical motor validation.

## Remaining integration

1. Add buffered whole-packet arbitration to the retained bridge, with priority
   STOP handling and explicit host heartbeat delivery. Keep all writes under
   `hx_safety` and retain the existing controller's travel/freshness checks.
2. Add verified trajectory storage/upload, versioned configuration and status,
   and map scheduler requests to existing telemetry/A1/audit operations.
3. Add a bounded timestamp/data stream and Rust acquisition adapter. Full 64-bit
   timestamps plus telemetry, audits and commanded PWM require at least 511 bytes
   per nine-axis frame before framing. At 40 ms this already exceeds a 115200-baud
   8N1 host link; budget a faster link or a validated compact encoding/storage mode.
   A stalled consumer must not silently discard evidence or disable stopping.
4. Exercise full UART/controller/supervisor tests, synthesize and place-and-route
   the integrated image, and re-run independent watchdog commissioning on hardware.
5. Measure zero-command cadence and data integrity, then bounded single/all-motor
   trajectories. Feed actual device-clock windows into the same Rust controller
   simulation. Retain new failures and compare against fresh physical recordings.
