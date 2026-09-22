# Current hardware commissioning and reload instructions

The user explicitly permits FPGA image loading with motor power on and authorizes
reloads as needed (September 14, 2026). Do not require a motor-power-off confirmation
for loading. The earlier restriction is superseded in `servo/safety.md`; historical
captures retain the policy that applied when they were made.

Use the exact verified supervised image and query its live profile after loading.
For development, use volatile SRAM:

```
openFPGALoader -b tangprimer25k examples/actuators/hx30hm/hardware/2026-09-14-full-drive/build/bridge_experiment_100pct.fs
```

Do not substitute the generic `make flash` target. The frozen image's SHA-256 is
`37d60ae1899569fbe349124addf741e3ae8182eb51e16fef6e14c24b1aee9a55`.
Expected profile: 1 Mbaud host and motor bus, 50 MHz device clock, capability 7,
fixed Q8 gains 4096/0/4096, and 0–1000 PWM on device-clock experiment plans.
Source archive/build timing remain frozen; authorization changes do not alter RTL.

Physical S2 stopping was verified on this exact image in `physical-s2-01/`, followed
by explicit rearm. After the latest powered reload (`powered-reload-01/`), live
profile verification, all three 5% ID12 watchdog checks, all-nine zero-drive device
recording and the all-nine short 25% reversal trial pass again. No wiring change
was reported. All nine final PWM/torque/speed readbacks are zero.

Current user-reported supply settings: 12.6 V / 3.5 A on WANPTEK DPS3010U, one
power daisy chain. Actual supply current and CV/CC state are unmeasured. The latest
matched 25% trial reaches 11.0 V minimum versus 10.9 V at 3 A; this one-increment
difference does not establish a current-limit cause or a wiring-current rating.
Higher-drive escalation remains held after the earlier 50% trial reached 9.1 V.

The acquisition command is:

```
HX_BAUD=1000000 target/debug/examples/characterize_hx_bridge \
  /dev/cu.usbserial-20250303171 1 4,5,6,7,8,9,10,11,12 NEW_CAPTURE PLAN.json
```

Use a fresh output directory on each attempt and retain failures. Check healthy
feedback, exact controller/PWM audits, zero/off readback and physical stationary
tail. Preserve all watchdog, thermal, voltage, raw-current, tracking/travel and
accuracy gates. Model fitting uses only completed real captures with independently
verified stopping. Training and validation roles remain separate; no new model
has yet been fitted to the higher-drive data.

Latest higher-stress follow-up: `stage-500-3p5a-01/` completes the same all-nine
±50% short reversal plan at the reported 3.5 A limit. All-nine stopping is verified.
Minimum voltage is still 9.1 V, matching the earlier 3 A trial. Higher-drive or
longer-duration escalation remains held; no controller or power setting changed.
