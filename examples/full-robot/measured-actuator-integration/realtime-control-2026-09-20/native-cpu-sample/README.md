# Native CPU sample

An eight-second macOS `sample` capture of the selected physical recipe identified
allocation/deallocation as a substantial cost. The reported collapsed leaf
table attributes 2167 of 5618 main-thread samples (38.57%) to
`libsystem_malloc.dylib`. This includes startup and simulation; it is not a
steady-state allocation-time measurement. Symbols with fewer than five samples
are omitted from that table, and sampling perturbs runtime. Do not use this
run's wall time in throughput comparisons.

Kinematics, rigid inertia and the motion-column constructor are visible hot
paths. Inspection showed that the shared rigid motion map builds one vector per
link, copies inherited columns, then grows the vector for the link's own joint
columns. `../contiguous-motion-storage/` tests replacing that nested storage
with one column buffer and link ranges, preserving column and arithmetic order.

The sampled run completed the same three-second trajectory. All 151 frame
fields except wall timing, and all task transitions, exactly match the existing
unsampled receipt. `protocol.json` identifies the binary and recipe;
`sample.txt` retains the full call graph and collapsed stacks, and `summary.json`
stores the parsed leaf counts and replay verification. Session 84589 completed.
