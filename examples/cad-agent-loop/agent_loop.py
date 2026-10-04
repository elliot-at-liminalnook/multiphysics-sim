#!/usr/bin/env python3
"""An agent working in the CAD editor through the viewer's REST API.

Run the viewer on a copy of a model (never on a file you care about):

    cp examples/wheeled-robot/baseline/robot.rcad /tmp/rover.rcad
    target/debug/sim-spatial /tmp/rover.rcad
    python3 examples/cad-agent-loop/agent_loop.py /tmp/rover.rcad

The script does what an agent does in a design review:

1. opens the model and reads its parts;
2. models a battery tray (box, cable hole cut with a cylinder, filleted top edges);
3. pins a comment on the tray as an agent ("AI" in the Comments dock);
4. saves a named, sliced view of the tray for the person to click
   (Saved Views panel → Restore view);
5. saves the file;
6. watches for the person's reply (type one in the Comments dock) and answers it.

Every edit is one undo step in the window (Ctrl+Z). Only the standard library.
"""
import json
import os
import sys
import time
import urllib.request

BASE = os.environ.get("SIM_REST", "http://127.0.0.1:8421")
AGENT = "Claude"


def call(command, **args):
    """One command; its value, or an exception with the viewer's own words."""
    body = json.dumps({"commands": [{"command": command, "args": args}]}).encode()
    req = urllib.request.Request(BASE + "/v1/batch", data=body, headers={"Content-Type": "application/json"}, method="POST")
    job = json.load(urllib.request.urlopen(req, timeout=10))["job_id"]
    while True:
        r = json.load(urllib.request.urlopen(f"{BASE}/v1/jobs/{job}", timeout=10))
        if r["status"] in ("succeeded", "failed", "cancelled"):
            result = r["results"][0]
            if not result["ok"]:
                raise RuntimeError(f"{command}: {result.get('error')}")
            return result["value"]
        time.sleep(0.1)


def main(path):
    sys.stdout.reconfigure(line_buffering=True)
    opened = call("cad_open", path=os.path.abspath(path))
    print("opened:", opened["message"])
    parts = {n["name"]: n["id"] for n in call("cad_state")["nodes"]}
    print("parts:", ", ".join(parts))

    # 2. Model: a tray above the chassis with a cable hole and soft top edges.
    tray = call("cad_model", op="box", name="Battery tray", corner=[-40, -25, 45], size=[50, 50, 6], material="petg")["result"]["id"]
    cutter = call("cad_model", op="cylinder", name="cable hole", base=[-15, 0, 40], axis=[0, 0, 1], radius=6, height=20)["result"]["id"]
    call("cad_model", op="cut", target=tray, tools=[cutter])
    topology = call("cad_model", op="topology", node=tray)["result"]
    top = [e["index"] for e in topology["edges"] if e["kind"] == "line" and e["start"][2] > 50.9 and e["end"][2] > 50.9]
    call("cad_model", op="fillet", node=tray, radius=2, edges=top)
    mass = call("cad_state")["local_mass"]["bodies"][tray]["mass_kg"]
    print(f"tray: {len(topology['faces'])} faces before the fillet, {mass * 1000:.1f} g of PETG (exact B-rep)")

    # 3. A comment pinned on the tray, written as an agent.
    thread = call("cad_threads", op="create", node=tray, point=[-15, 12, 51],
                  body=f"I added a 6 mm PETG battery tray with a 12 mm cable hole ({mass * 1000:.0f} g). "
                       "Is 6 mm enough, or should it carry the battery's weight on ribs?",
                  author=AGENT, author_kind="agent")["result"]["id"]

    # 4. A named view that slices through the hole, showing only the tray and chassis.
    view = call("cad_views", op="save", name="Battery tray: cable hole section",
                description="Cut through the cable hole: tray thickness, hole edge and the fillet on the top face.",
                fit=[tray], section={"axis": "y", "offset": 0}, parts=[tray] + ([parts["chassis"]] if "chassis" in parts else []),
                author=AGENT, author_kind="agent")["result"]
    print("saved view:", view["name"])

    # 5. Save the file (atomic write).
    print("saved:", call("cad_save")["saved"])

    # 6. Watch for the person's reply and answer it.
    print("waiting for a reply in the Comments dock (Ctrl+C stops)…")
    call("cad_threads", op="seen", reader=AGENT)
    while True:
        news = call("cad_threads", op="watch", reader=AGENT, author_kind="person", timeout_s=20)
        for c in news["comments"]:
            if c["thread"] != thread:
                continue
            print(f"{c['author']}: {c['body']}")
            answer = (f"Thanks — noted: “{c['body'][:120]}”. I'll keep the tray at 6 mm for now; "
                      "restore “Battery tray: cable hole section” to see the wall around the hole.")
            call("cad_threads", op="reply", thread=thread, body=answer, author=AGENT, author_kind="agent")
            call("cad_save")
            print("answered and saved")
            return


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
