//! CAD-derived ideal mechanism sweep; this is not a dynamic simulation.
use serde_json::json;
use sim_domain_robot::articulated::embedding::{EmbeddingConfig, RigidEmbedding};
use sim_runtime::session::{Scene, Session};
use std::time::Instant;

#[derive(serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SweepConfig {
    embedding: EmbeddingConfig,
    initial_coordinates: Option<Vec<f64>>,
    amplitude_rad: f64,
    duration_s: f64,
    expected_cad_sha256: Option<String>,
    initial_base_translation_m: Option<[f64;3]>,
    trajectory: Option<sim_domain_control::trajectory::TrajectoryConfig>,
}
impl Default for SweepConfig {
    fn default() -> Self {
        Self {
            embedding: EmbeddingConfig::default(),
            initial_coordinates: None,
            amplitude_rad: 0.05,
            duration_s: 1.0,
            expected_cad_sha256: None,
            initial_base_translation_m: None,
            trajectory: None,
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args.len() > 3 {
        return Err("usage: audit_embedding scene.json [samples=41] [sweep.json]".into());
    }
    let n: usize = args
        .get(1)
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| "invalid sample count")?
        .unwrap_or(41);
    if !(3..=10001).contains(&n) {
        return Err("samples must be 3..10001".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let session = Session::new(scene, 0)?;
    let art = &session.robot.art;
    let names: Vec<String> = session
        .scene
        .robot
        .motors
        .iter()
        .map(|m| {
            let joint = m
                .joint
                .as_ref()
                .ok_or_else(|| format!("motor {} has no declared joint", m.name))?;
            let found: Vec<_> = art
                .dofs()
                .filter(|(j, _)| &j.name == joint)
                .map(|(_, d)| d.name.clone())
                .collect();
            if found.len() != 1 {
                return Err(format!(
                    "motor {} does not select exactly one coordinate",
                    m.name
                ));
            }
            Ok(found[0].clone())
        })
        .collect::<Result<_, String>>()?;
    if names.is_empty() {
        return Err("no motor coordinates".into());
    }
    let sweep: SweepConfig = args
        .get(2)
        .map(|p| {
            serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        })
        .transpose()?
        .unwrap_or_default();
    if !sweep.amplitude_rad.is_finite() || sweep.amplitude_rad <= 0.0 {
        return Err("sweep amplitude must be positive finite radians".into());
    }
    if sweep.expected_cad_sha256.as_ref().is_some_and(|hash|Some(hash.as_str())!=session.scene.robot.source.get("cad_sha256").and_then(|s|s.as_str())) {return Err("sweep CAD hash mismatch".into());}
    if !sweep.duration_s.is_finite() || sweep.duration_s<=0.0 {return Err("positive finite sweep duration required".into());}
    let trajectory=sweep.trajectory.clone().map(sim_domain_control::trajectory::Trajectory::new).transpose()?;
    if trajectory.as_ref().is_some_and(|t|t.dimension()!=names.len()) {return Err("trajectory dimension must match named motors".into());}
    let config = sweep.embedding;
    let map = RigidEmbedding::new(art, &names, config.clone())?;
    let mut seed = session.robot.generalized();
    if let Some(delta)=sweep.initial_base_translation_m {
        if art.bases.len()!=1 || art.bases[0].grounded || delta.iter().any(|v|!v.is_finite()) {return Err("finite translation of one floating base required".into());}
        for k in 0..3 {seed.states[art.bases[0].state+k]+=delta[k];}
    }
    let initial: Vec<_> = sweep.initial_coordinates.unwrap_or_else(|| {
        map.independent_joint_indices()
            .iter()
            .map(|i| seed.q[*i])
            .collect()
    });
    let nb = map.reduced_dimension() - names.len();
    let omega = std::f64::consts::TAU;
    let mut frames = Vec::new();
    let mut solve_seconds = 0.0;
    let mut inertia_seconds = 0.0;
    let mut dynamics_seconds = 0.0;
    let mut derivative_audit_seconds = 0.0;
    for frame in 0..n {
        let t = sweep.duration_s * frame as f64 / (n - 1) as f64;
        let mut q: Vec<_> = initial
            .iter()
            .enumerate()
            .map(|(i, q)| {
                q + sweep.amplitude_rad * (omega * t).sin() * (0.7 * i as f64 + 0.3).sin()
            })
            .collect();
        let mut velocity = vec![0.0; nb];
        velocity.extend((0..names.len()).map(|i| {
            sweep.amplitude_rad * omega * (omega * t).cos() * (0.7 * i as f64 + 0.3).sin()
        }));
        if let Some(trajectory)=&trajectory {
            let sample=trajectory.sample(t)?;q=sample.values;
            velocity[nb..].copy_from_slice(&sample.rates);
        }
        let start = Instant::now();
        let motion = map
            .solve(&seed, &q, &velocity)
            .map_err(|e| format!("sample {frame} at {t}s: {e}"))?;
        solve_seconds += start.elapsed().as_secs_f64();
        if let Some((i,(_,d)))=art.dofs().enumerate().find(|(i,(_,d))|d.lower.is_some_and(|v|motion.generalized.q[*i]<v) || d.upper.is_some_and(|v|motion.generalized.q[*i]>v)) {
            return Err(format!("prescribed sample {frame} at {t}s violates authored joint limit: {} = {}, bounds {:?}..{:?}",d.name,motion.generalized.q[i],d.lower,d.upper));
        }
        let start = Instant::now();
        let direct = art.rigid_closure_velocity_jacobian(&motion.generalized)?;
        let reference = art.audit_constraints(
            &motion.generalized,
            &sim_domain_robot::articulated::constraints::RankConfig {
                length_scale_m: config.length_scale_m,
                angle_scale_rad: config.angle_scale_rad,
                ..Default::default()
            },
        )?;
        let mut jacobian_error = 0.0_f64;
        for i in 0..direct.nrows() {
            for j in 0..direct.ncols() {
                jacobian_error = jacobian_error.max(
                    (direct[(i, j)] * reference.column_scales[j] / reference.row_scales[i]
                        - reference.scaled_velocity_matrix[i][j])
                        .abs(),
                );
            }
        }
        if !jacobian_error.is_finite() || jacobian_error > 1e-10 {
            return Err(format!(
                "sample {frame}: direct closure Jacobian disagrees with original velocity probes: {jacobian_error}"
            ));
        }
        derivative_audit_seconds += start.elapsed().as_secs_f64();
        let start = Instant::now();
        let mass = art.rigid_mass_matrix(&motion.generalized)?;
        let reduced = motion.tangent.transpose() * &mass * &motion.tangent;
        if reduced.clone().cholesky().is_none() {
            return Err("projected mass is not positive definite".into());
        }
        inertia_seconds += start.elapsed().as_secs_f64();
        let start = Instant::now();
        let dynamics = map.accelerations(&motion, &vec![0.0; map.full_dimension()])?;
        dynamics_seconds += start.elapsed().as_secs_f64();
        // Re-evaluate the original dynamics independently of reduced assembly.
        // Verification work is excluded from the kernel timing above.
        let check = art.evaluate(&dynamics.generalized);
        let required = nalgebra::DVector::from_iterator(
            map.full_dimension(),
            art.bases
                .iter()
                .enumerate()
                .filter(|(_, b)| !b.grounded)
                .flat_map(|(i, _)| check.base_wrench[i])
                .chain(
                    check
                        .joints
                        .iter()
                        .flat_map(|j| j.tau_needed.iter().zip(&j.tau_passive).map(|(a, b)| a - b)),
                ),
        );
        let projected_force_error = (motion.tangent.transpose() * required).amax();
        if !projected_force_error.is_finite() || projected_force_error > 1e-7 {
            return Err(format!(
                "original dynamics disagrees at sample {frame}: {projected_force_error}"
            ));
        }
        let rows = art.original_closure(&motion.generalized);
        frames.push(json!({"time_s":t,"motor_coordinates":q,
            "joint_positions":motion.generalized.q,"iterations":motion.position_iterations,
            "maximum_scaled_closure_jacobian_error":jacobian_error,
            "minimum_scaled_singular_value":motion.minimum_scaled_singular_value,
            "maximum_scaled_position_error":motion.maximum_scaled_position_error,
            "maximum_scaled_velocity_error":motion.maximum_scaled_velocity_error,
            "maximum_scaled_acceleration_error":motion.maximum_scaled_acceleration_error,
            "zero_applied_load_accelerations":dynamics.reduced_accelerations.as_slice(),
            "original_projected_force_error":projected_force_error,
            "original_rows":rows,
            "internal_contacts":check.contacts.iter().filter(|c|c.other.is_some()).map(|c|json!({"link":c.link,"other":c.other,"penetration_m":c.penetration,"force_n":c.force.as_slice(),"point_m":c.point.as_slice()})).collect::<Vec<_>>(),
            "poses":art.poses(&motion.generalized).iter().zip(&art.model.links).map(|((r,p),l)|
                json!({"name":l.name,"position_m":p.as_slice(),"rotation":(0..3).map(|i|(0..3).map(|j|r[(i,j)]).collect::<Vec<_>>()).collect::<Vec<_>>()})).collect::<Vec<_>>()
        }));
        seed = motion.generalized;
    }
    println!("{}",serde_json::to_string_pretty(&json!({"version":1,"completed":true,
        "source":session.scene.robot.source,"options":session.scene.options,"embedding":config,
        "independent_coordinates":names,"full_velocity_dimension":map.full_dimension(),
        "reduced_velocity_dimension":map.reduced_dimension(),"samples":n,
        "kinematic_mapping_s":solve_seconds,"inertia_projection_s":inertia_seconds,
        "trajectory":sweep.trajectory,"duration_s":sweep.duration_s,"initial_base_translation_m":sweep.initial_base_translation_m,
        "closure_derivative_audit_s":derivative_audit_seconds,"amplitude_rad":sweep.amplitude_rad,"initial_coordinates":initial,
        "instantaneous_dynamics_s":dynamics_seconds,"frames":frames,
        "notes":["Prescribed geometric sweep of motor coordinates, not actuator-driven physics or a walking test.",
            "Ideal original closure replaces stabilized/CFM constraints only in this diagnostic; all rows remain checked.",
            "Retains every link mass/inertia through T-transpose M T. Separate instantaneous zero-applied-load dynamics includes shared contact/passive loads; it does not integrate time or contact history.",
            "Direct closure derivatives are checked against independent original unit-velocity probes at every sampled state; audit time is excluded from kernel timings.",
            "Recorded motor-coordinate amplitudes do not establish complete travel, collision clearance or branch-global validity."]
    })).map_err(|e|e.to_string())?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
