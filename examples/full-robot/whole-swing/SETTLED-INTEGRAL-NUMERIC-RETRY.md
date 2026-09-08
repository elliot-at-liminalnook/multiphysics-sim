# Retry the identical controller cases after numeric binding repair

The initial three cases in `settled-integral-plan.json` all stopped with zero
recorded physics steps: Rhai's typed deserializer rejected integer-valued
numeric fields such as `leak_rate_per_s: 0`. The original code and failures
are retained at commit f352159 and in `settled-integral-binding-failure.json`.

Normalize integer scalar parameters to floats before the shared typed config
is deserialized. Keep strings, booleans, unknown fields and invalid bounds
rejected. Add a mixed-integer/float JSON regression alongside exact kernel
agreement and failure rollback. The Rust update equations do not change.

After these checks and a rebuilt native runner, execute the exact same three
scene/config/action/task files in a fresh `settled-integral-numeric` directory.
All gains, bias/rate bounds, seed, task gates and pre-stop identity requirements
remain those in `SETTLED-INTEGRAL-PLAN.md`. This is a binding repair, not tuning.
