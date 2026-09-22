# Autonomous transaction integration milestone

The scheduler, packet builder and raw-reply adapter now run together. The core
requires both the matching raw telemetry and independent supervisor acceptance,
records the exact transmitted PWM words, and compares torque/PWM readbacks with
that batch. Completed event data survives stopping and logger backpressure.
Controller gains are frozen at START; no new controller arithmetic is introduced.

**Verification:** nine integrated transaction groups, seven reply-parser groups
and sixteen scheduler groups pass. Integrated Gowin synthesis passes. The tests
use a packet-level bridge model, not actual UART bits or physical motors.

**Not yet complete:** bridge arbitration and sealed-row wiring, validated START
identity, timestamp/start/terminal transport, Rust acquisition, complete UART
verification, final integrated place-and-route, and hardware commissioning.
Nothing in this milestone has been loaded onto the FPGA. Existing physical
tracking/model errors remain unchanged.

See the [interface contract and limitations](experiment-transactions.md).
The [source/log manifest](manifest.json) uses SHA-256. Reproduce from this directory
with the hardware toolchain on PATH:

```
make sim-experiment-transactions sim-experiment-reply sim-experiment-scheduler
make check-experiment-transactions
```

The build targets do not program a board. The copied Makefile also lists other
hardware-repository targets whose unrelated sources are not included here.
