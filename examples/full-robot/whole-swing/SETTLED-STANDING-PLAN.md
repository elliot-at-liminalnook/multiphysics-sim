# Delay stronger feedback until the planned transfer has stopped

The stronger standing experiment failed while its final transfer was still
moving. Preserve the original teacher's 0.5 standing increment during that
motion. Permit an additional 0.75 only after all joint reference angles have
remained unchanged within 1e-8 rad for 0.4 s and motion commands are zero.
The existing measured-support multiplier remains active. This uses controller
reference history, not invented hardware observations or a change to physics.

Repeat the original 5/2.5 ms teacher minute with this policy. Keep every moving
gain, physical bound, posture, action, task, seed and accuracy budget unchanged.
The Rhai policy continues using shared Rust body/point corrections. Its extra
gain must not change any target during an active transfer; verify from captured
planner phase, observations and actual motor targets that changed gain occurs
only in idle frames. Require a real effect during idle and retain all physical
and numerical failures. This remains privileged teacher development.
