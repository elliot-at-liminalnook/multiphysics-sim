# simloop (C)

`simloop.h` is a single-header C99 client for the simulator's external-controller
seam over stdin/stdout, with no dependencies.  Define `SIMLOOP_IMPLEMENTATION`
in exactly one translation unit before including it.

## Build the example

    cc -std=c99 -Wall -Wextra -Werror -Iclients/c -o p_controller clients/c/examples/p_controller.c

## Use

```c
#define SIMLOOP_IMPLEMENTATION
#include "simloop.h"

simloop_t loop;
if (simloop_open(&loop, stdin, stdout) != 0) return 1;      /* reads hello, sends ready; loop.error on failure */
/* loop.element, loop.period, loop.n_sensors, loop.sensor_names[i], loop.sensor_units[i],
   loop.n_actuators, loop.actuator_names[i], loop.actuator_units[i] */
simloop_frame_t f; double act[SIMLOOP_MAX_CHANNELS] = {0};
while (simloop_next(&loop, &f) == 1) {                      /* 1 = frame, 0 = closed/EOF, -1 = error */
    act[0] = -2.0 * f.sensors[0];                            /* f.t is simulation time, f.seq the sample number */
    simloop_send(&loop, &f, act);                            /* exactly loop.n_actuators values */
}
```

`simloop_sensor_index` / `simloop_actuator_index` look channels up by name.
Limits: `SIMLOOP_MAX_CHANNELS` (64) per direction, names and units under 64 bytes,
frames under 16 KiB.  Doubles are written with `%.17g`.  Never write to stdout
yourself; log to stderr.

## Tests

    sh clients/c/tests/run.sh          # exact-output check against a scripted conversation
    python3 clients/c/tests/interop.py # a Python fake simulator drives the compiled example

## Protocol

Newline-delimited JSON, lockstep, simulator first (see `crates/sim-couple`):

    -> {"type":"hello","element":"controller","period":0.001,"sensors":[{"name":"angle","unit":"rad"}],"actuators":[{"name":"voltage","unit":"V"}]}
    <- {"type":"ready"}
    -> {"type":"sample","seq":0,"t":0.0,"sensors":[0.1]}
    <- {"type":"act","seq":0,"actuators":[2.5]}
    -> {"type":"close"}

## In-process controllers (shared libraries)

`examples/quadruped_gait_dl.c` is the trot as a shared library: it exports
`simloop_open(element, period, n_sensors, n_actuators)`,
`simloop_sample(t, sensors, n, actuators, m)` and (optionally)
`simloop_close()`; the simulator loads it with `sim_couple::DynamicCoupler`
and calls it in-process — no pipe, no JSON, the cost of a Rust closure.
Build by hand with

    cc -O2 -std=c99 -shared -fPIC -o libquadruped_gait.dylib examples/quadruped_gait_dl.c -lm

or let `DynamicCoupler::compile(source, library)` do it when the source is
newer than the library. Any language with a C ABI can export the same three
symbols. Channels arrive sorted by name.
