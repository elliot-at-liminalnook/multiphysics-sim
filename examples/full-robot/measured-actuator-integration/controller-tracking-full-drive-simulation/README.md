# Full-drive offline controller experiment

Simulation only. The motors were disconnected; no hardware limit was changed.
See the [complete report](../controller-tracking-simulation/README.md) for results,
methods, numerical checks, limitations, and reproduction commands.

- [Original versus tuned tracking](tracking-comparison.png)
- [Command limits versus original-gait error](reference-envelope.png)
- [Fast governed reference traces](governed-reference.png)
- `selected-gains.json`: training-selected controller, not a deployment approval.
- `validation.json`: all 80 original/selected validation cases and raw-trace links.
- `robustness/summary.json`: independent checks, including shared supply.
- `reference-envelope/summary.json`: speed/acceleration tradeoff, retaining error
  to the original gait as well as error to the changed command.

The selected full-drive controller still fails to reproduce the original full-size
gait. Good tracking of a limited command does not prove that the modified gait walks.
