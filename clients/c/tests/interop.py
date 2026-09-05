#!/usr/bin/env python3
"""Play the simulator against the compiled C example: hello, 5 samples, close."""

import json
import math
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
HELLO = {"type": "hello", "element": "controller", "period": 0.01, "sensors": [{"name": "angle", "unit": "rad"}], "actuators": [{"name": "voltage", "unit": "V"}]}

with tempfile.TemporaryDirectory() as build:
    exe = os.path.join(build, "p_controller")
    subprocess.check_call(["cc", "-std=c99", "-Wall", "-Wextra", "-Werror", "-I" + os.path.join(HERE, ".."), "-o", exe, os.path.join(HERE, "..", "examples", "p_controller.c")])
    child = subprocess.Popen([exe, "3"], stdin=subprocess.PIPE, stdout=subprocess.PIPE)

    def exchange(frame):
        child.stdin.write((json.dumps(frame) + "\n").encode()); child.stdin.flush()
        line = child.stdout.readline()
        assert line, f"controller closed its stdout after {frame}"
        return json.loads(line)

    assert exchange(HELLO) == {"type": "ready"}
    angle, acts = 0.5, []
    for seq in range(5):
        reply = exchange({"type": "sample", "seq": seq, "t": seq * HELLO["period"], "sensors": [angle]})
        assert reply["type"] == "act" and reply["seq"] == seq, reply
        assert len(reply["actuators"]) == 1 and all(math.isfinite(v) for v in reply["actuators"]), reply
        assert reply["actuators"][0] == -3 * angle, reply
        angle += 0.1 * reply["actuators"][0]
        acts.append(reply["actuators"][0])
    child.stdin.write(b'{"type":"close"}\n'); child.stdin.close()
    assert child.wait(5) == 0, f"controller exited {child.returncode}"
    print(f"ok: 5 samples, acts={acts}")
