Latest status: the user confirmed motor power off; the frozen image was loaded
into SRAM successfully and the 1 Mbaud profile was verified. Power-on was confirmed. All-nine inspection, three 5% ID12 watchdog trials, and
a 12-frame all-nine zero-drive device capture now pass. The physical S2 motion-stop
test is next; the operator must confirm readiness to press S2 after motion starts. The checklist below
preserves the original complete sequence; load/profile steps are complete.

# Hardware commissioning handoff

At preparation time no image had been loaded and no motor had been driven.
The prior image was a 115200-baud, 10% controller. The successful new image uses 1,000,000 baud on both links.

Before loading, obtain an explicit physical confirmation that motor supply power
is off while the FPGA remains USB powered. This is required by the hardware
project's `servo/safety.md` under “Validation and reconnection.” Torque-off
telemetry is not motor power-off. Load the frozen `.fs` into SRAM only, using
`openFPGALoader -b tangprimer25k <exact-frozen-image>`; do not use the generic
`make flash` target or write persistent flash.

After loading, verify the new profile and fixed-gain/autonomous capability 7.
Restore motor power with the operator, inspect all nine IDs 4–12, and verify the
small-drive physical S2 stop, independent host-loss and telemetry-loss stops,
explicit rearm, and encoder stationarity. Existing software `safety_probe`
exercises loss of traffic; it cannot press S2 or establish a physical cable break.
Keep the supply power-cut available if telemetry/stop delivery fails.

Use the exact bitstream-bound plans, with a new output directory on every trial:

```
HX_BAUD=1000000 target/debug/examples/characterize_hx_bridge \
  /dev/cu.usbserial-20250303171 1 4,5,6,7,8,9,10,11,12 NEW_CAPTURE PLAN.json
```

First establish all-nine zero-drive device cadence. Advance finite reversal
admission pulses through 25%, 50%, 75%, and 100%, inspecting the full retained
capture and physical stop each time. The cap is permitted PWM, not measured
maximum speed. Readbacks must establish which motors actually saturated.
Retain trips and stop on lost feedback; do not relax tracking, travel, thermal,
voltage, raw-current, watchdog, or validation thresholds to obtain a completed run.

Training and validation templates are distinct and must remain so. Fit only
completed real captures with audited controller outputs and observed stopping.
Report encoder timing resolution, unresolved steady-speed tails, voltage,
temperature and raw current. Freeze individual model candidates before fresh
all-nine confirmation; keep the prior accepted model when accuracy gates fail.

The S2 test command (only after operator readiness) is the acquisition command above
with `physical-s2.json` and a fresh output directory. It runs ID12 at ±5%, changes
direction every 200 ms, and expires after eight seconds. The other eight motors
remain off. After successful S2 stopping and button release, zero-drive rearm and
the staged all-nine plans are the next steps. No new firmware reload is needed.
