#!/usr/bin/env python3
"""Play the simulator against a controller child process.

    python3 fake_sim.py [command ...]     (default: the PI example)

Sends hello and five samples, checks five acts with matching seq and finite
values, sends close and waits for the child to exit 0.
"""

import json
import math
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT = [sys.executable, os.path.join(HERE, "..", "examples", "pi_controller.py"), "--kp", "2", "--ki", "5", "--setpoint", "1", "--limit", "3"]
HELLO = {"type": "hello", "element": "controller", "period": 0.01, "sensors": [{"name": "speed", "unit": "rad/s"}], "actuators": [{"name": "voltage", "unit": "V"}]}


def main(command):
    env = dict(os.environ, PYTHONPATH=os.path.join(HERE, "..") + os.pathsep + os.environ.get("PYTHONPATH", ""))
    child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=env)

    def exchange(frame):
        child.stdin.write((json.dumps(frame) + "\n").encode()); child.stdin.flush()
        line = child.stdout.readline()
        assert line, f"controller closed its stdout after {frame}"
        return json.loads(line)

    assert exchange(HELLO) == {"type": "ready"}
    speed, acts = 0.0, []
    for seq in range(5):
        reply = exchange({"type": "sample", "seq": seq, "t": seq * HELLO["period"], "sensors": [speed]})
        assert reply["type"] == "act" and reply["seq"] == seq, reply
        assert len(reply["actuators"]) == 1 and all(math.isfinite(v) for v in reply["actuators"]), reply
        speed += 0.1 * reply["actuators"][0]  # a toy plant
        acts.append(reply["actuators"][0])
    child.stdin.write(b'{"type":"close"}\n'); child.stdin.close()
    assert child.wait(5) == 0, f"controller exited {child.returncode}"
    print(f"ok: 5 samples, acts={acts}")


if __name__ == "__main__":
    main(sys.argv[1:] or DEFAULT)
