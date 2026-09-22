# CAD power profiles in the shared runtime

CAD now owns versioned battery parameters, radial power branches and an explicit
operating envelope. Stable motor IDs resolve to physical joint names; units,
provenance, evidence and uncertainty survive the existing command/undo/archive/
export path. Unknown/missing IDs, cycles, duplicate motor assignments, incorrect
units and simultaneous legacy/CAD supplies fail validation.

Explicit session selection enables the same registered battery, bridge and
resistor laws in both reference and incremental runtime paths. The environment
exposes terminal voltage/current/power/SOC/energy and named branch observations.
Replay and host chunking preserve motor and supply states. Envelope violations
reject an entire interval; checks occur at endpoints and command jumps, not at
every continuous crossing. This is a model-validity envelope, not a physical BMS.

Runtime integration suites: 36 passing tests. CAD profile suite: 5 passing tests.
The first reference-circuit test incorrectly assumed current must be lower after
adding wiring resistance; changed motor speed/back-EMF makes that comparison
invalid. The corrected test checks KCL and voltage drop against R*I at every
reference step. The failed run is retained with the final passing run.

These use synthetic declared power parameters and prove software behavior. The
quadruped CAD revision has not been assigned measured battery/wiring values and
continues to use explicitly imposed supply voltages. Power integration does not
establish electrical or loaded-leg calibration. Gait generation is the current
priority, independently of the historical 0.264° bench target.

`checks.json` and source hashes record the scope of completed verification.
The earlier `../power-coupling/` evidence remains a separate core-only snapshot.
