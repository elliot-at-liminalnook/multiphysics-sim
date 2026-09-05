"""Export CAD, run the generic sim-cad plant, and enforce acceptance budgets.

Build sim-cad in release mode first. Compilation is excluded from timing.
"""
import argparse
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

from build_model import build

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def read(path):
    return json.loads(Path(path).read_text())


def finite(value):
    if isinstance(value, dict):
        return all(finite(v) for v in value.values())
    if isinstance(value, list):
        return all(finite(v) for v in value)
    return not isinstance(value, float) or math.isfinite(value)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def verify(model, ref, runs, logs, limits, elapsed):
    checks = []

    def check(name, value, bound, minimum=False):
        passed = math.isfinite(value) and (value >= bound if minimum else value <= bound)
        checks.append({"name": name, "observed": value if math.isfinite(value) else None, "bound": bound,
                       "comparison": ">=" if minimum else "<=", "passed": passed})

    bar = next(l for l in model["links"] if l["name"] == "pendulum")
    check("finite CAD model", int(not finite(model)), 0)
    check("CAD mass relative error", abs(bar["mass"] / ref["mass_kg"] - 1), limits["mass_relative_error"])
    check("CAD inertia relative error", max(abs(bar["inertia"][k][k] / ref["inertia_kg_m2"][k] - 1) for k in range(3)), limits["inertia_relative_error"])
    check("CAD inertia off-diagonal", max(abs(bar["inertia"][i][j]) for i in range(3) for j in range(3) if i != j), 1e-12)
    check("CAD centre of mass", max(abs(a-b) for a, b in zip(bar["com"], ref["com_m"])), limits["geometry_absolute_error_m"])
    joint = next(j for j in model["joints"] if j["name"] == "pivot")
    check("CAD pivot", max(abs(a-b) for a, b in zip(joint["origin"], ref["pivot_m"])), limits["geometry_absolute_error_m"])
    check("CAD motor wired to pivot", int(model["motors"][0]["joint"] != "pivot"), 0)
    check("CAD gravity", max(abs(a-b) for a,b in zip(model["gravity"], [0,0,-ref["gravity_m_s2"]])), 1e-12)

    for name, run in runs.items():
        check(name + " finite results", int(not finite(run)), 0)
        trace = run["trace"]
        ts = trace["t"]
        angle = trace["joints"]["pivot"]
        motor = trace["motors"]["servo"]
        check(name + " duration", abs(run["duration_s"] - limits["duration_s"]), 1e-9)
        check(name + " sample count", abs(len(ts) - round(limits["duration_s"] / limits["sample_s"])), 0)
        check(name + " sample cadence", max(abs(t - (k+1)*limits["sample_s"]) for k,t in enumerate(ts)), 1e-9)
        check(name + " trace lengths", max(abs(len(v)-len(ts)) for v in [angle, *motor.values()]), 0)
        check(name + " step refinements", run["step_refinements"], limits["maximum_step_refinements"])
        check(name + " peak current", max(abs(i) for i in motor["current"]), limits["peak_current_a"])
        frames = logs[name]
        # A frame at t=duration is allowed by event-location rounding.
        count = round(limits["duration_s"] / limits["controller_period_s"])
        check(name + " controller frame count", min(abs(len(frames)-count), abs(len(frames)-count-1)), 0)
        check(name + " controller cadence", max(abs(f["t"]-k*limits["controller_period_s"]) for k,f in enumerate(frames)), 1e-9)
        check(name + " controller sequence", max(abs(f["seq"]-k) for k,f in enumerate(frames)), 0)
        check(name + " finite controller log", int(not finite(frames)), 0)
        # Cross-check the controller's sensor frames against the plant trace.
        # Skip t=0, which precedes the first 10 ms result sample.
        observed = {round(t / limits["sample_s"]): q for t,q in zip(ts, angle)}
        check(name + " controller sensor agreement", max(abs(f["angle_rad"] - observed[round(f["t"] / limits["sample_s"])]) for f in frames if f["t"] > 1e-9), 1e-6)
        check(name + " controller reference", max(abs(f["target_rad"] - (0.0 if name == "hold" or f["seq"] < 10 else (0.3 if f["seq"] < 80 else -0.2))) for f in frames), 1e-12)
        if name == "hold":
            errors = [abs(angle[k]-target) for lo,hi,target in limits["settled_windows_s"] for k,t in enumerate(ts) if lo <= t <= hi]
            check("hold fails the tracking criterion", min(errors), limits["falsifier_minimum_tracking_error_rad"], minimum=True)
        else:
            check(name + " motor energized", max(abs(i) for i in motor["current"]), limits["minimum_peak_current_a"], minimum=True)
            for lo, hi, target in limits["settled_windows_s"]:
                indices = [k for k,t in enumerate(ts) if lo-1e-9 <= t <= hi+1e-9]
                if not indices:
                    raise ValueError(f"{name}: missing settled window {lo}..{hi}")
                check(f"{name} settled tracking at {target} rad", max(abs(angle[k]-target) for k in indices), limits["settled_angle_error_rad"])
                # About the Y hinge, the motor must balance m*g*r*sin(q).
                errors = [motor["torque_nm"][k] - ref["mass_kg"]*ref["gravity_m_s2"]*ref["com_radius_m"]*math.sin(angle[k]) for k in indices]
                check(f"{name} mean static torque at {target} rad", abs(sum(errors)/len(errors)), limits["static_torque_error_nm"])
        check(name + " wall budget", elapsed[name], limits["run_budget_s"])

    nominal = runs["nominal"]["trace"]
    for name, bound in [("repeat", limits["repeat_trace_absolute_error"]), ("half_step", limits["step_halving_angle_difference_rad"])]:
        other = runs[name]["trace"]
        if len(nominal["t"]) != len(other["t"]):
            raise ValueError("cannot compare traces of different lengths")
        check(name + " sample time agreement", max(abs(a-b) for a,b in zip(nominal["t"], other["t"])), 1e-9)
        check(name + " angle agreement", max(abs(a-b) for a,b in zip(nominal["joints"]["pivot"], other["joints"]["pivot"])), bound)
        for signal in ["current", "winding_c", "torque_nm"]:
            diffs = [a-b for a,b in zip(nominal["motors"]["servo"][signal], other["motors"]["servo"][signal])]
            if name == "repeat":
                check("repeat " + signal, max(abs(d) for d in diffs), bound)
            elif signal == "current":
                check("half-step current RMS difference", math.sqrt(sum(d*d for d in diffs)/len(diffs)), limits["step_halving_current_rms_difference_a"])
    check("CAD export wall budget", elapsed["export"], limits["export_budget_s"])
    check("total wall budget", elapsed["total"], limits["total_budget_s"])
    return checks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sim", type=Path, default=ROOT / "target/release" / ("sim-cad.exe" if os.name == "nt" else "sim-cad"))
    parser.add_argument("--output", type=Path, default=ROOT / "runs/motorized-pendulum")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    limits = read(HERE / "expectations.json")
    started = time.perf_counter()
    report = {"version": 1, "passed": False, "limits": limits, "checks": [],
              "platform": platform.platform(), "python": sys.version,
              "model_sha256": None, "sim_sha256": None,
              "controller_sha256": digest(HERE / "controller.py"), "commands": []}
    report["source_sha256"] = {p.name: digest(p) for p in [HERE / "build_model.py", HERE / "run.py", HERE / "expectations.json", ROOT / "Cargo.lock"]}
    report["packages"] = {name: importlib.metadata.version(name) for name in ["cadquery-ocp", "numpy", "scipy", "trimesh", "PySide6"]}
    report["timing_excludes"] = ["Rust compilation", "dependency installation"]
    try:
        sim = args.sim.resolve()
        if not sim.is_file():
            raise ValueError("build the release binary first: cargo build --release --bin sim-cad")
        report["sim_sha256"] = digest(sim)
        model, reference = build(output)
        elapsed = {"export": time.perf_counter() - started}
        model_path = output / "pendulum.simrobot.json"
        report["model_sha256"] = digest(model_path)
        runs, logs = {}, {}
        # Fixed nominal repeat, finer discretisation, and a controller falsifier.
        for name, mode, step in [("nominal", "track", limits["step_s"]),
                                 ("repeat", "track", limits["step_s"]),
                                 ("half_step", "track", limits["step_s"] / 2),
                                 ("hold", "hold", limits["step_s"])]:
            result = output / (name + ".simresult.json")
            log = output / (name + ".controller.jsonl")
            command = [str(sim), "run", str(model_path), "--seconds", str(limits["duration_s"]),
                       "--step", str(step), "--no-flex", "--no-contact", "--out", str(result),
                       "--controller", str(HERE / "controller.py"), "--python", sys.executable,
                       "--controller-arg=--mode", "--controller-arg=" + mode,
                       "--controller-arg=--log", "--controller-arg=" + str(log)]
            report["commands"].append(command)
            before = time.perf_counter()
            with (output / (name + ".log")).open("w") as stream:
                subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT,
                               check=True, timeout=limits["subprocess_timeout_s"])
            elapsed[name] = time.perf_counter() - before
            runs[name] = read(result)
            logs[name] = [json.loads(line) for line in log.read_text().splitlines()]
            print(f"{name}: {elapsed[name]:.2f} s wall", flush=True)
        elapsed["total"] = time.perf_counter() - started
        report["elapsed_s"] = elapsed
        report["checks"] = verify(model, reference, runs, logs, limits, elapsed)
        report["passed"] = all(c["passed"] for c in report["checks"])
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
    (output / "acceptance.json").write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    failures = [c for c in report["checks"] if not c["passed"]]
    for c in failures:
        print(f"FAIL {c['name']}: {c['observed']} {c['comparison']} {c['bound']}")
    if "error" in report:
        print(report["error"], file=sys.stderr)
    print(f"{'PASS' if report['passed'] else 'FAIL'}: {len(report['checks'])} checks; {output / 'acceptance.json'}")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
