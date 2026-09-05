# simloop (Python)

A stdlib-only client (Python 3.9+) for the simulator's external-controller seam.

## Install

Add `clients/python` to `PYTHONPATH`; there are no dependencies.

    export PYTHONPATH=/path/to/physics-simulator/clients/python

## Use

```python
import sys
from simloop import Loop

loop = Loop.stdio()                           # the simulator spawned us and speaks over stdin/stdout
# loop = Loop.listen(("127.0.0.1", 9000))     # or: accept one simulator connection over TCP
# loop = Loop.listen_unix("/tmp/sim.sock")    # or: over a Unix socket
c = loop.contract                             # .element, .period, .sensors / .actuators as Channel(name, unit)
print(c, file=sys.stderr)                     # never print to stdout in stdio mode
for frame in loop:                            # frame.t (sim time), frame.seq, frame["angle"], frame.sensors, frame.values
    loop.send(voltage=-2.0 * frame["angle"])  # by name, or loop.send([v]) by position; unmentioned actuators hold
```

`Loop` raises `simloop.ProtocolError` on a malformed frame or seq mismatch and ends iteration on `close` or EOF.

## Example

    python3 examples/pi_controller.py --kp 4 --ki 20 --setpoint 1 --sensor speed --actuator voltage --limit 12

Point the simulator at that command line; `tests/fake_sim.py` plays the simulator for a smoke test.

## Tests

    python3 -m unittest discover -s clients/python/tests -t clients/python
    python3 clients/python/tests/fake_sim.py

## Protocol

Newline-delimited JSON, lockstep, simulator speaks first (see `crates/sim-couple`):

    -> {"type":"hello","element":"controller","period":0.001,"sensors":[{"name":"angle","unit":"rad"}],"actuators":[{"name":"voltage","unit":"V"}]}
    <- {"type":"ready"}
    -> {"type":"sample","seq":0,"t":0.0,"sensors":[0.1]}
    <- {"type":"act","seq":0,"actuators":[2.5]}
    -> {"type":"close"}

Each `act` echoes the sample's `seq` and carries exactly as many actuators as the hello declared.

## From the simulator side

`sim_couple::python(clients_root, script, args)` spawns a script with this
directory on `PYTHONPATH`, and `Runtime::attach_python(behavior,
clients_root, script, args)` attaches it to a `control.external` element in
one call. Give negative-valued flags as `--flag=value`.
