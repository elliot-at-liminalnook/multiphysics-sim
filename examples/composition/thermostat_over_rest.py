#!/usr/bin/env python3
"""The composition workflow in the running app, over REST (the same handlers
as Build mode's controls; docs/architecture/composition.md):

    cargo build -p sim-spatial -p sim-fmi
    target/debug/sim-spatial                      # REST on :8421 (SIM_REST for another)
    python3 examples/composition/thermostat_over_rest.py [DIR]

1. Packs the thermostat FMU from its C source into DIR/fmus (sim-fmi pack).
2. Writes an empty system file DIR/room.system.json and opens it in Build mode.
3. Places a room from ordinary thermal parts (air, wall, outside, heater,
   thermometer) with one `system` batch.
4. Inspects the FMU, adds it as a block (system_add_fmu), wires it
   (thermometer → thermostat → heater) and changes its clock
   (set_block_timing): every edit validated and undoable.
5. Saves an acceptance test, runs it on a background thread and prints each
   requirement's outcome and where the evidence stands.
6. Starts the live run for a few seconds and prints its state.

Only the standard library. Writes only into DIR (default
<workspace>/target/composition-rest, replaced if it exists).
"""
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request

BASE = os.environ.get("SIM_REST", "http://127.0.0.1:8421")
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def call(command, timeout=600, **args):
    body = json.dumps({"commands": [{"command": command, "args": args}]}).encode()
    req = urllib.request.Request(BASE + "/v1/batch", data=body, headers={"Content-Type": "application/json"}, method="POST")
    job = json.load(urllib.request.urlopen(req, timeout=10))["job_id"]
    t0 = time.time()
    while time.time() - t0 < timeout:
        r = json.load(urllib.request.urlopen(f"{BASE}/v1/jobs/{job}", timeout=10))
        if r.get("status") in ("succeeded", "failed", "cancelled"):
            x = r["results"][0]
            if not x["ok"]:
                raise SystemExit(f"{command}: {x.get('error')}")
            return x["value"]
        time.sleep(0.2)
    raise TimeoutError(command)


def element(name, component_type, x, **parameters):
    return {"command": "add_instance", "name": name, "instance": {
        "kind": {"kind": "element", "component_type": component_type},
        "parameters": {k: {"value": v} for k, v in parameters.items()},
        "placement": {"position": [x, 0.0, 0.0]}}}


def connect(*terminals):
    return {"command": "connect", "terminals": [{"instance": i, "port": p} for i, p in terminals]}


def main():
    out = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target", "composition-rest"))
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(os.path.join(out, "fmus"))
    built = os.path.join(ROOT, "target", "debug", "sim-fmi")
    packer = [built] if os.path.exists(built) else ["cargo", "run", "-q", "-p", "sim-fmi", "--bin", "sim-fmi", "--"]
    subprocess.run(packer + ["pack", os.path.join(ROOT, "examples", "fmi", "thermostat"), os.path.join(out, "fmus", "thermostat.fmu")], check=True, cwd=ROOT)
    system = os.path.join(out, "room.system.json")
    with open(system, "w") as f:
        json.dump({"schema": "sim.system/2", "title": "Thermostat room", "revision": 0, "root": "root",
                   "definitions": {"root": {"label": "Thermostat room"}}}, f)

    print("open", call("viewer_mode", mode="build", path=system))
    time.sleep(2)
    call("system", label="Room", commands=[
        {"command": "set_run_settings", "run": {"integrator": "backward_euler", "interval": 0.5}},
        element("outside", "thermal.ambient", 0.0, temperature=278.15),
        element("air", "thermal.capacitance", 0.25, heat_capacity=2.0e4, **{"initial.temperature": 290.15}),
        element("wall", "thermal.conductance", 0.5, conductance=8.0),
        element("heater", "thermal.controlled_heat_source", 0.75),
        element("thermometer", "thermal.temperature_sensor", 1.0),
        connect(("air", "node"), ("wall", "a"), ("heater", "node"), ("thermometer", "node")),
        connect(("wall", "b"), ("outside", "node")),
    ])
    fmu = call("system_inspect_fmu", path="fmus/thermostat.fmu")
    print("FMU", fmu["model_name"], "inputs", [(v["name"], v["kind"] or v["kind_error"]) for v in fmu["inputs"]],
          "outputs", [(v["name"], v["kind"] or v["kind_error"]) for v in fmu["outputs"]], "unsupported", fmu["unsupported"])
    added = call("system_add_fmu", path="fmus/thermostat.fmu", name="thermostat", period=0.5, parameters={"setpoint": 294.15})
    print("added", added["added"])
    call("system", label="Wire the thermostat", commands=[
        connect(("thermometer", "temperature"), ("thermostat", "temperature")),
        connect(("thermostat", "heater_power"), ("heater", "power")),
        {"command": "set_block_timing", "name": "thermostat", "timing": {"clock": {"kind": "periodic", "period": 1.0}}},
    ])
    # The scene recompiles off the UI thread after an edit: wait for it.
    while (state := call("system_state"))["compiling"]:
        time.sleep(0.5)
    print("compile_error", state["compile_error"], "findings", [f["message"] for f in state["findings"]])

    test = {"duration_s": 3600, "requirements": [
        {"id": "comfort", "observable": "thermometer.temperature", "reduce": "mean", "window": [1800, 3600], "min": 293.6, "max": 294.7},
        {"id": "rating", "observable": "thermostat.heater_power", "reduce": "max", "max": 500}]}
    call("system_test", action="set", name="comfort", test=test)
    call("system_test", action="run", name="comfort")
    while (c := call("system_state")["composition"])["running"]:
        time.sleep(1)
    last = c["last"]["result"]
    if "Err" in last:
        raise SystemExit(f"test: {last['Err']}")
    evidence = last["Ok"]
    print("test", evidence["verdict"], [(r["id"], r["status"], r["detail"]) for r in evidence["results"]])
    print("standing", c["standing"])

    call("system_run", action="start")
    time.sleep(4)
    live = call("system_state")["live_run"]
    call("system_run", action="pause")
    print("live run t =", live["time"], "phase", live["phase"], "error", live["error"])


if __name__ == "__main__":
    main()
