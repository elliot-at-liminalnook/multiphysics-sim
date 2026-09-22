# Nonideal gait scenarios

See [scenario plan](PLAN.md). The initial two cases alter only the CAD profile's
command-delivery delay (10 ms) or internal gearbox play (0.5°). Both are estimated
sensitivity probes, not hardware measurements. Physical unit assignments stay
unset. The parent full-authority CAD archive and all previous runs are preserved.

`cad/scripts/prepare_profile_scenario.py` applies each full profile declaration via
the normal CAD command and native Rust validator. Save/reload and unchanged geometry
checks produce receipts. `export_simrobot.py --no-flex` matches the parent export;
`../prepare_profile_search.mjs` verifies physical export equality before rebinding
the scenario to the exact parent runtime representation and historical commands.

The delay case's first export omitted `--no-flex`. It is retained as
`delay-10ms/rejected-flex.simrobot.json` with its original receipt/log, and is not
used for gait comparison. The corrected export explicitly matches the parent.

Battery/wiring, synthetic motor variation, stale-feedback behavior and thermal
state integration remain subsequent work. No robustness claim follows from
preparing these scenarios; execution and numerical checks are required.
