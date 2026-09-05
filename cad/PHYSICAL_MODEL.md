# Physical assembly description (simrobot v3)

The CAD tool derives a **physical description** of a robot from the `.rcad`
document and the simulator treats it as a coupled multiphysics model. Python
owns geometry and derivation; Rust owns dynamics. Nothing is hand-declared
that geometry and materials can provide.

File: `<name>.simrobot.json`, `"version": 3`. **SI units throughout** (m, kg,
s, rad, N, Pa, V, A, K or °C where the key says `_c`). v2 (mm, planar) stays
readable by the planar path; v3 is what the CAD tool writes.

Index conventions: matrices are row-major nested lists; the SDF grid is a
flat list with `index = (ix * ny + iy) * nz + iz`; quaternions are `[w, x, y, z]`.

## Frames

* **World frame** = the CAD frame at the pose the file was exported in, z up
  unless `gravity` says otherwise, scaled to metres.
* **Link frame** = origin at the link's centre of mass, axes parallel to the
  world axes at the export pose. Everything under a link (`inertia`,
  `collision`, `flex`, sensor/cable `point`s, joint `origin`s that are given
  per link) is in this frame.
* A joint's `origin` and `axis` are given in the **world frame at export pose**
  (Rust converts to parent and child link frames using the links' `com`).

## Top level

```
{
  "version": 3,
  "source": {"file": "leg.rcad", "exported": "2026-09-03T12:00:00"},
  "gravity": [0, 0, -9.81],
  "world": {"floor_z": 0.0, "floor_friction": 0.8, "floor_stiffness": 2e5, "floor_damping": 2e3,
            "terrain": null | {"origin": [x, y], "cell": m, "dims": [nx, ny], "heights": [flat]}},
  "materials": {id: Material},
  "links": [Link], "joints": [Joint], "motors": [Motor],
  "battery": Battery | null, "sensors": [Sensor], "cables": [Cable],
  "control": Control, "uncertainty": Uncertainty,
  "identification": {joint_name: {...}} | {},        // fitted overrides, see System identification
  "planar": {"normal": [3], "origin": [3]} | null     // hint for a planar projection
}
```

### Material
```
{"id", "name", "density", "youngs_modulus", "poisson", "yield_strength", "ultimate_strength",
 "glass_transition_c", "thermal_conductivity", "specific_heat", "thermal_expansion",
 "friction": {other_material_id: {"static": mu_s, "kinetic": mu_k}},   // includes itself and "steel", "world"
 "print": {"anisotropy_z": 0.6, "layer_adhesion_factor": 0.7} | null}
```
`youngs_modulus` etc. are the bulk values; `print.anisotropy_z` scales the
modulus and strength across layers. Density kg/m³.

### Link
```
{"name", "id", "members": [node ids merged into this link], "material": id, "ground": bool,
 "mass", "com": [3] (world, export pose), "inertia": [[3x3]] about com in link axes,
 "bbox": [[min3],[max3]] (link frame),
 "collision": {
    "vertices": [[x,y,z]...] (link frame, ≤ 3000, surface-sampled incl. sharp corners),
    "triangles": [[i,j,k]...],
    "hull": [[x,y,z]...] (convex hull vertices, broad phase),
    "sdf": {"origin": [3], "cell": m, "dims": [nx,ny,nz], "values": [flat signed distances, negative inside]}},
 "flex": null | {
    "normalization": "mass_normalized",  // phiᵀ M phi = I; amplitude m·√kg, rate m·√kg/s
    "modes": m, "frequencies_hz": [m], "damping_ratio": 0.03,
    "boundary_frames": [{"id": stable attachment ID, "name": joint or attachment name, "point": [3] (link frame),
        "role": "root"|"outboard"|"attachment", "patch": {"radius_m", "radius_source": "inferred"|"declared"|"mesh_default",
        "selection": "within_radius"|"nearest_available_nodes", "node_count", "bounds_m": [[min3],[max3]]}}],
    "modal_stiffness": [m] (= (2π f)² for mass-normalised modes), "modal_mass": [m] (1.0),
    "boundary_shapes": [[m][nb][6]] displacement (3) and small rotation (3) of each boundary frame per unit modal coordinate,
    "participation": [[m][6]] modal force per unit link-frame acceleration (3 linear, 3 angular): ∫ρ φ·(1, r×) dV,
    "stress_cells": [[x,y,z]...] (element centroids, link frame, ≤ 2000),
    "stress_per_mode": [[m][ncells][6]] stress tensor (xx,yy,zz,xy,yz,xz) per unit modal coordinate,
    "gravity_sag_m": maximum static nodal displacement norm, worst of three link-frame 1-g axes,
    "softening": {"tg_c": 60, "width_c": 10, "ratio_above": 0.05}},   // E(T) = E · (ratio + (1-ratio)·(1 - sigmoid((T - tg)/width)))
 "print": {"orientation": [3] (build z in link frame), "infill": 0.3, "walls": 3, "layer_height": 0.2e-3}}
```
Mesh vertex count and SDF resolution are chosen by the exporter (cell ≈ 1–2 mm, ≤ 48³).
Custom modal bases may declare `displacement` normalization (amplitudes in metres).
Missing normalization is `unspecified` and exposes amplitudes as `modal`, with a
warning. It does not change the numeric modal basis. Experiment traces additionally
record world-space attachment positions and physical displacement in metres;
replay uses scaled boundary arrows alongside rigid CAD meshes.

### Joint
```
{"name", "id", "type": "revolute"|"continuous"|"prismatic"|"fixed"|"ball"|"loop_revolute"|"loop_spherical",
 "parent": link name | null (world), "child": link name,
 "origin": [3] (world), "axis": [3] unit, "limits": [lo, hi] | null, "home": rad,
 "physics": {
    "source": "inferred"|"declared", "pin_radius", "hole_radius", "contact_length",
    "flex_patch_radius": m, "flex_patch_source": "inferred"|"declared",
    "clearance": radial m, "backlash": rad (≈ clearance / lever radius, from geometry),
    "wobble": rad (angular play from clearance/contact_length),
    "friction": {"coulomb": N·m, "viscous": N·m·s/rad, "stribeck": N·m, "stribeck_speed": rad/s, "static_ratio": 1.2},
    "stiffness": {"radial": N/m, "axial": N/m, "bending": N·m/rad}, "damping_ratio": 0.05,
    "bearing": {"kind": "printed_pin"|"ball_bearing"|"servo_horn"|"bolt", "allowable_pressure": Pa}},
 "fastened": null | {"screw": "M3", "count": n, "preload": N, "stiffness": N/m, "shear_capacity": N, "pattern_radius": m},
 "motor": motor name | null}
```
Tree joints (`revolute|continuous|prismatic|fixed|ball`) must form a forest;
`loop_*` joints close loops and are solved as constraints. Prismatic
`limits` are metres. Fixed joints are compliant (`stiffness`, `fastened`).

### Motor
```
{"name", "id", "spec": library id, "joint": name | null, "mounted_on": link name, "mount_point": [3] world, "shaft_axis": [3],
 "gear_ratio": extra ratio declared on the joint,
 "electrical": {"resistance", "inductance", "torque_constant", "back_emf_constant", "no_load_current",
                "rotor_inertia" (rotor side), "supply_voltage", "current_limit", "poles": 0 | n},
 "gearbox": {"ratio", "efficiency", "backlash_rad" (output side), "inertia" (output side), "stiffness": N·m/rad (gear train), "max_output_torque", "max_output_speed"},
 "thermal": {"winding_heat_capacity", "case_heat_capacity", "r_winding_case", "r_case_mount", "r_case_ambient", "resistance_temp_coeff": 0.0039, "torque_derating_per_c": 0.001, "max_winding_c"},
 "firmware": {"kind": "servo"|"position"|"velocity"|"torque"|"stepper"|"none", "loop_rate_hz", "latency_s", "deadband_rad", "sensor_resolution_rad", "kp", "ki", "kd", "output": "voltage"|"current"},
 "driver": {"kind": "h_bridge"|"servo_internal"|"stepper"|"esc", "pwm_hz", "on_resistance", "current_limit"}}
```

### Battery, Sensor, Cable, Control, Uncertainty
```
Battery: {"cells", "nominal_voltage", "internal_resistance", "capacity_ah", "initial_soc": 1.0, "cutoff_voltage"}
Sensor:  {"name", "id", "kind": "imu"|"encoder"|"current"|"force", "link", "point": [3] link frame, "axes": [[3x3]] rows = sensor x,y,z in link frame,
          "rate_hz", "noise": {"accel": σ m/s², "gyro": σ rad/s, "angle": σ}, "bias": {"accel": [3], "gyro": [3]}, "bias_walk": σ/√s,
          "quantization": {"angle": rad, "accel": m/s²}, "joint": name (encoder/current), "range": {"accel": g, "gyro": rad/s}}
Cable:   {"name", "id", "from": {"link", "point"}, "to": {"link", "point"}, "length", "mass", "stiffness": N (EA), "damping", "segments": 4}
Control: {"period_s", "latency_s", "targets": {joint: rad}, "mode": "hold"|"trajectory", "trajectory": [{"t", "targets": {...}}]}
Uncertainty: {"dimension_m": {"sigma"}, "mass": {"sigma_fraction"}, "friction": {"sigma_fraction"}, "stiffness": {"sigma_fraction"},
              "backlash": {"sigma_fraction"}, "motor_torque": {"sigma_fraction"}, "com_m": {"sigma"}, "seed": 0}
```

## Results file (Rust → CAD)

`<name>.simresult.json` written beside the model after every run:
```
{"version": 1, "model": path, "duration_s", "steps", "wall_s", "warnings": [str],
 "links": {name: {"peak_stress_pa", "yield_margin" (yield/peak - 1), "hotspot": {"cells": [[xyz] link frame], "stress_pa": []},
                  "max_deflection_m", "peak_temperature_c", "tg_margin_c"}},
 "joints": {name: {"peak_reaction_force_n", "peak_reaction_torque_nm", "bearing_pressure_pa", "bearing_margin",
                   "screw_shear_margin" | null, "range_used_rad": [lo, hi], "limit_hits", "friction_loss_j", "backlash_crossings"}},
 "motors": {name: {"peak_current_a", "rms_current_a", "peak_torque_nm", "stall_margin", "peak_winding_c", "peak_mount_c",
                   "mount_tg_margin_c", "energy_j", "saturated_fraction"}},
 "battery": {"final_soc", "min_voltage", "energy_j"} | null,
 "base": {"fell": bool, "final_pose": {"position": [3], "quaternion": [4]}, "path": [[t, x, y, z]]},
 "contacts": {"peak_force_n", "pairs": [[a, b, peak_force]]},
 "monte_carlo": null | {"samples", "seed", "metrics": {name: {"mean", "std", "p5", "p50", "p95"}}, "success_rate"},
 "trace": {"t": [...], "joints": {name: [angle...]}, "motors": {name: {"current": [...], "winding_c": [...]}}}}
```
The CAD tool reads it (`ops.load_results`) to paint stress, list margins in
the outliner/properties and the Robot panel, and serve it at `GET /results`.

## System identification

`sim-cad fit <model.simrobot.json> <log.csv> [--out fitted.json]`. Log columns:
`t`, `<joint>.angle`, `<joint>.target` (rad) and optionally `<motor>.current`,
`<motor>.voltage`. The fitter adjusts per-joint `friction.coulomb`,
`friction.viscous`, `backlash`, `stiffness` scale and motor `torque_constant`
to minimise trajectory (and current) error, and writes an `identification`
block: `{joint: {"friction": {...}, "backlash", "stiffness_scale", "rms_error_rad", "source_log", "fitted_at"}}`.
The CAD tool imports it (`ops.apply_identification(path)`); the exporter
copies it into the next model file, and Rust applies it over the inferred values.

## Rust layout

* `crates/sim-domain-robot` — elements registered in `sim_phenomena::world::registry`:
  * `robot.articulated`: the whole body tree (floating base + tree joints + loop constraints + modal flex + geometry contact against floor/terrain, other links and itself). Model data is passed by handle: `sim_domain_robot::register_model(PhysicalModel) -> f64 handle`, param `model`. Ports: `joint.<name>` (Rotational for revolute/continuous, Translational for prismatic), `frame.<name>` (Frame) for every sensor/cable attachment and `frame.base`, signal_in `temperature.<link>`, signal_out `contact.<link>` (normal force sum).
    Params: `gravity`, `planar` (0/1 projects onto `planar` hint), `contact.*` overrides, `flex` (0/1), `initial.joint.<name>.angle/speed`.
  * `robot.motor_unit`: electrical `p`/`n`, rotational `shaft` (gearbox output), thermal `winding`, signal_out `current`, `torque`, `speed`; R(T), L, kt, ke, rotor inertia, gearbox ratio/efficiency/backlash/stiffness, no-load loss, derating; I²R → winding.
  * `robot.servo_firmware`: signal_in `target`, `measured`; signal_out `command` (voltage or current); sampled loop with latency, deadband, quantised sensor, PID with saturation.
  * `robot.h_bridge`: `supply_p/supply_n` in, `p/n` out, signal_in `command`; averaged, on-resistance, current limit.
  * `robot.battery`: `p/n`; SOC state, EMF(SOC), internal resistance, cutoff.
  * `robot.thermal_probe`: thermal `node` → signal_out `temperature`.
  * `robot.imu`: Frame `mount` → signals `ax ay az gx gy gz` (sampled, noise, bias walk, quantisation).
  * `robot.cable`: Frame `a`, Frame `b`; lumped elastic cable, tension-only.
* `sim-phenomena::scenarios::cad_robot`: v3 builder wiring all of the above into one ModelWorld (`CadRobot::build`), planar projection option, Monte Carlo (`run_monte_carlo`), results writer, `fit` for identification. v2 files keep the old planar chain path.
* `sim-cad` CLI: `sim-cad run <model> [--seconds s] [--planar] [--montecarlo N] [--out results.json]`, `sim-cad fit <model> <log.csv>`; bare `sim-cad <model> [seconds]` still works.
* `sim-app --scene cad --model <file>`: 3D meshes from the collision meshes posed by the articulated element, stress colouring (key S), contacts and joint axes as gizmos, file watch as before.
