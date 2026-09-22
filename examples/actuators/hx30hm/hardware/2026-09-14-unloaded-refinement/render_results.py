"""Plot retained measurements and Rust predictions; no controller or plant model.
Run: uv run --with matplotlib python PATH/render_results.py
"""
import json
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parent
DEGREES_PER_COUNT = 360 / 4096

def read(path):
    return json.loads((ROOT / path).read_text())

def measured(recording, motor):
    observations = [next(o for o in f["observations"] if o["id"] == motor) for f in recording["frames"]]
    times = [(o["request_s"] + o["completion_s"]) / 2 for o in observations]
    angles = [(o["telemetry"]["position_raw"] - recording["home"][motor - 4]) * DEGREES_PER_COUNT for o in observations]
    return times, angles, observations

plt.rcParams.update({"font.size": 10, "axes.spines.top": False, "axes.spines.right": False})
trial = "confirm-mixed-10pct"
recording = read(f"{trial}/fpga-recording.json")
baseline = {p["id"]: p for p in read(f"{trial}-baseline-closed-loop.json")["predictions"]}
candidate = {p["id"]: p for p in read(f"{trial}-candidate-closed-loop.json")["predictions"]}
fig, axes = plt.subplots(3, 3, figsize=(13, 9), sharex=True, sharey=True, layout="constrained")
for motor, ax in zip(range(4, 13), axes.flat):
    t, angle, _ = measured(recording, motor)
    ax.plot(t, angle, "o-", color="#173b54", markersize=2.5, lw=1.5, label="Actual motor encoder")
    for label, result, color in [("Previous model", baseline[motor], "#aaa29b"), ("Per-motor candidate", candidate[motor], "#da7734")]:
        samples = result["samples_time_encoder_duty_angle"]
        ax.plot([s[0] for s in samples], [(s[1] - recording["home"][motor - 4]) * DEGREES_PER_COUNT for s in samples], color=color, lw=1.3, label=label)
    ax.set_title(f"Motor {motor} · prediction RMS {baseline[motor]['rms_prediction_degrees']:.2f}° → {candidate[motor]['rms_prediction_degrees']:.2f}°")
    ax.grid(alpha=.18)
for ax in axes[-1]: ax.set_xlabel("Time (s)")
for ax in axes[:, 0]: ax.set_ylabel("Relative angle (°)")
axes[0, 0].legend(fontsize=8)
fig.suptitle("Fresh all-nine physical run · 10% drive ceiling\nSame integer controller, simulated using its own feedback; models frozen before capture", fontsize=14)
fig.savefig(ROOT / "fresh-prediction.png", dpi=160)
fig.savefig(ROOT / "fresh-prediction.svg")
plt.close(fig)

fig, axes = plt.subplots(2, 1, figsize=(11, 7), sharex=True, layout="constrained")
for name, label, color in [("shared-check-id4", "ID4 moves; others hold", "#6599bd"), ("shared-all", "All nine move", "#dc7734"), ("shared-check-id4-repeat", "ID4-only repeat", "#264859")]:
    r = read(f"{name}/fpga-recording.json")
    t, angle, obs = measured(r, 4)
    axes[0].plot(t, angle, "o-", ms=2, label=label, color=color)
    axes[1].plot(t, [o["telemetry"]["voltage_v"] for o in obs], "o-", ms=2, color=color)
axes[0].set_ylabel("ID4 relative angle (°)")
axes[1].set_ylabel("ID4 servo voltage (V)")
axes[1].set_xlabel("Time from capture start (s)")
axes[0].legend()
for ax in axes: ax.grid(alpha=.18)
fig.suptitle("Shared-load comparison · same ID4 targets and all-nine polling cadence\nVoltage resolution 0.1 V; current is uncalibrated; one moving-axis repeat pair", fontsize=13)
fig.savefig(ROOT / "shared-load-comparison.png", dpi=160)
fig.savefig(ROOT / "shared-load-comparison.svg")
plt.close(fig)

motion = read("train-sync-10pct-motion.json")
fig, axes = plt.subplots(2, 1, figsize=(11, 7), sharex=True, layout="constrained")
for motor in range(4, 13):
    m = motion["axes"][str(motor)]
    for ax, key in zip(axes, ["speed_rad_s", "acceleration_rad_s2"]):
        samples = m[key]
        ax.plot([s["time_s"] for s in samples], [s["value"] * 180 / 3.141592653589793 for s in samples], label=f"ID{motor}", lw=1)
axes[0].set_ylabel("Sampled speed (°/s)")
axes[1].set_ylabel("Sampled acceleration (°/s²)")
axes[1].set_xlabel("Time (s)")
axes[0].legend(ncol=9, fontsize=8)
for ax in axes: ax.grid(alpha=.18)
fig.suptitle("Nine real motors reversing together · 10% drive ceiling\nRust estimates from encoder windows, nominal 150 ms cadence; sample age unknown", fontsize=13)
fig.savefig(ROOT / "measured-speed-acceleration.png", dpi=160)
fig.savefig(ROOT / "measured-speed-acceleration.svg")
plt.close(fig)
