# Existing quadruped with a linked leg

`robot.rcad` preserves the original baseline geometry, body IDs, joint records,
physical metadata and names. Its **Leg +X** group is now an occurrence of the
embedded **Quadruped leg +X** component. `quadruped-leg.rcomp` makes that assembly
portable; placement requires an explicit chassis connection.

The other three legs are retained as originally modeled. Their part counts and
structures differ, so replacing them with rotated copies of +X would be a design
change, not a verified lossless conversion. `verification.json` records the source
hash and the conversion checks. The original `examples/full-robot/baseline/robot.rcad`
is unchanged.

Imported solids are reusable immediately. Design dimensions must be exposed with
explicit recipes; they are not reverse-engineered from the existing B-reps.
