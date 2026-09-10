# A conservative bound for the recorded loaded-slip measure

This derivation supplies a contact-schedule-free optimization diagnostic using
recorded normal forces and horizontal material velocities. It is now implemented
as a [shared Rust component and optional planner objective](SLIP_OBJECTIVE.md);
it has not been deployed as an online control constraint.

For foot j at sample k, let f_p >= 0 be each point's normal force, v_p its
horizontal material velocity, N = sum_p f_p, and w_k > 0 the sample's integration
weight. Let T = sum_k w_k, D > 0 the net horizontal displacement, and L >= D the
sampled body XY path length. The development slip measure uses N0 = 1 N:

    u_k = sum_p f_p ||v_p|| / N_k
    S_j = sum_{k: N_k >= N0} w_k u_k / L

Define the dimensionless diagnostic

    B_j² = (T / D²) sum_k w_k
             [sum_p f_p ||v_p||² / max(N_k, N0)].

Then **S_j <= B_j** on those samples. Weighted Cauchy–Schwarz over the contact
points gives u_k² <= sum_p f_p ||v_p||² / N_k for loaded samples. Applying
Cauchy–Schwarz over their positive time weights gives

    [sum_loaded w_k u_k]² <= T sum_loaded w_k u_k².

For loaded samples max(N_k,N0) = N_k. Adding the nonnegative unloaded terms and
using L >= D proves the bound. The definition avoids division by zero without
introducing stance flags or prescribed touchdown times.

One possible residual representation uses two horizontal components per point:

    r_{p,k} = sqrt(T w_k f_p / [D² max(N_k,N0)]) * v_p
    B_j² = sum_{p,k on foot j} ||r_{p,k}||².

The maximum over feet bounds the existing worst-foot slip statistic. This
calculation applies to the flat, world-Z-up planning reports used here. Other
terrain would require consistent surface normals and tangential velocities.

## Evidence and limits

`summarize_basis_restoration.mjs` computes both S and B from the independently
audited 512 samples, and asserts the inequality for every foot. The largest
values are:

| Candidate | Measured sampled S | Conservative B |
| --- | ---: | ---: |
| 32 controls, previous 112-point search | .899064 | 1.694251 |
| 32 controls, new 144-point search | .894836 | 1.705176 |
| 64 controls, new 144-point search | .898953 | 1.696627 |

This confirms that better balance has barely changed sliding in this basin.
The shared Rust implementation now uses B as an optional shaping residual while
retaining the actual slip acceptance measure. Its analytic tests cover zero
slip, unloaded swing, constant slip and the weighted inequality.

B <= .05 would be sufficient, but **not necessary**, for the sampled S <= .05
requirement. The bound can penalize variation in speed and lightly loaded swing
that the original measure does not count. Do not silently replace the original
acceptance gate with this stronger surrogate, or interpret failure to satisfy
the surrogate as proof of a physical speed ceiling.

The inequality proves a relationship between sampled diagnostics only. It does
not certify unobserved times, the detailed runtime, hardware contact, stability,
collision freedom, or maximum achievable speed. Fixed positive displacement is
required; a future task with zero displacement needs a different normalization.
