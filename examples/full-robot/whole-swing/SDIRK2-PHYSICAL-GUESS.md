# Start the second mechanical solve from the first physical stage

The optional stage audit locates the coarse teacher jump in the second solve.
The affine seed reaches 5.37 rad/s at the foot servo, while Newton converges
to 247.62 rad/s after large corrections. Tighter tolerances preserve that root.
The vector SDIRK primitive already starts the second solve from the first
physical stage; the mechanical adapter currently starts from the affine seed.

Add a reusable mechanical implicit solve with an explicit reduced-velocity
guess, validated for dimension and finiteness. Leave the residual seed, forces,
closures, tolerances and acceptance checks unchanged. Use the first stage's
velocity as the second SDIRK solve's initial guess. Record that guess in the
existing optional endpoint audit. This is a solver change to the experimental
SDIRK adapter, not a new force law or a clipping limit. The original failed
algorithm remains reproducible at commit 7d593c6; backward Euler stays unchanged.

Run the 19 mechanical and 22 linkage tests, including analytic stage identities,
invalid-guess rejection and audit preservation. Then run the original 24-second
teacher/student SDIRK2 20 ms steering recipes and a rebuilt default-off student
20 ms reference. Require exact previous backward-Euler physics/task identity.
Keep all original swing, stopping, heading, tilt and collision gates. Preserve
every outcome before deciding whether a new 10/5 ms refinement or stage profile
is informative. No browser promotion without the original trajectory and
rendered performance gates.
