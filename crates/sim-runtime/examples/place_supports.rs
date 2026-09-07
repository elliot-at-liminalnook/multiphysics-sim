//! Reproducible geometric support placement using the shared mechanism map.
//! Does not alter CAD, integrate dynamics, or claim static force equilibrium.
use serde::{Deserialize, Serialize};
use serde_json::json;
use sim_domain_robot::articulated::embedding::{
    CoordinateInterval, EmbeddedPoint, EmbeddingConfig, PlanePlacementConfig, PointPlaneTarget,
    RigidEmbedding,
};
use sim_domain_robot::{Articulated, Generalized};
use sim_runtime::session::{Scene, Session};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    expected_cad_sha256: String,
    independent_coordinates: Vec<String>,
    bounds: Vec<CoordinateInterval>,
    support_links: Vec<String>,
    target_height_m: f64,
    #[serde(default)]
    initial_coordinates: Option<Vec<f64>>,
    #[serde(default)]
    embedding: EmbeddingConfig,
    #[serde(default)]
    placement: PlanePlacementConfig,
}

fn lowest_points(
    art: &Articulated,
    g: &Generalized,
    links: &[usize],
) -> Result<Vec<EmbeddedPoint>, String> {
    let kin = art.evaluate_kinematics_only(g);
    links
        .iter()
        .map(|&link| {
            let k = &kin[link];
            let local = art.links[link]
                .contact
                .iter()
                .min_by(|a, b| (k.p + k.r * *a).z.total_cmp(&(k.p + k.r * *b).z))
                .ok_or_else(|| {
                    format!("no compiled contact points for {}", art.links[link].name)
                })?;
            Ok(EmbeddedPoint {
                link,
                local_point_m: [local.x, local.y, local.z],
            })
        })
        .collect()
}

fn run() -> Result<bool, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: place_supports scene.json placement.json".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let config: Config =
        serde_json::from_slice(&std::fs::read(&args[1]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if scene
        .robot
        .source
        .get("cad_sha256")
        .and_then(|v| v.as_str())
        != Some(config.expected_cad_sha256.as_str())
    {
        return Err("placement CAD hash does not match scene".into());
    }
    if !config.target_height_m.is_finite() || config.support_links.is_empty() {
        return Err("finite target height and support links required".into());
    }
    let session = Session::new(scene, 0)?;
    let art = &session.robot.art;
    let map = RigidEmbedding::new(
        art,
        &config.independent_coordinates,
        config.embedding.clone(),
    )?;
    let links = config
        .support_links
        .iter()
        .map(|name| {
            let matches: Vec<_> = art
                .links
                .iter()
                .enumerate()
                .filter(|(_, l)| &l.name == name)
                .map(|(i, _)| i)
                .collect();
            if matches.len() == 1 {
                Ok(matches[0])
            } else {
                Err(format!("support link is missing or ambiguous: {name}"))
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    if links
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != links.len()
    {
        return Err("duplicate support link".into());
    }
    let seed = session.robot.generalized();
    let initial = config.initial_coordinates.clone().unwrap_or_else(|| {
        map.independent_joint_indices()
            .iter()
            .map(|i| seed.q[*i])
            .collect()
    });
    let mut motion = map.solve(&seed, &initial, &vec![0.0; map.reduced_dimension()])?;
    let report = |g: &Generalized| -> Result<serde_json::Value, String> {
        let points = lowest_points(art, g, &links)?;
        let q: Vec<_> = map
            .independent_joint_indices()
            .iter()
            .map(|i| g.q[*i])
            .collect();
        let (_, values) = map.point_jacobians(g, &q, &points)?;
        let poses = art.poses(g);
        Ok(json!({
            "contacts":art.evaluate(g).contacts.iter().map(|c| json!({
                "link":art.links[c.link].name,"other":c.other.map(|i|art.links[i].name.clone()),
                "penetration_m":c.penetration,"force_n":c.force.as_slice()
            })).collect::<Vec<_>>(),
            "coordinates": q,
            "supports": points.iter().zip(values).map(|(p, (world, jac))| json!({
                "link":art.links[p.link].name, "mass_kg":art.links[p.link].mass,
                "local_point_m":p.local_point_m, "world_point_m":world.as_slice(),
                "floor_clearance_m":world.z - art.model.world.floor_z,
                "height_error_m":world.z - config.target_height_m,
                "height_derivatives_per_coordinate":jac.row(2).iter().copied().collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
            "joint_positions":g.q,
            "poses":poses.iter().zip(&art.links).map(|((r,p),l)| json!({"name":l.name,"position_m":p.as_slice(),
                "rotation":(0..3).map(|i|(0..3).map(|j|r[(i,j)]).collect::<Vec<_>>()).collect::<Vec<_>>()})).collect::<Vec<_>>()
        }))
    };
    let before = report(&motion.generalized)?;
    let mut fits = Vec::new();
    let mut failure = None;
    let mut completed = false;
    // Rotation can change the lowest sampled surface vertex. Re-select it and
    // verify all support minima, rather than treating one fixed marker as a sole.
    for _ in 0..8 {
        let targets = lowest_points(art, &motion.generalized, &links)?
            .into_iter()
            .map(|point| PointPlaneTarget {
                point,
                normal_world: [0.0, 0.0, 1.0],
                offset_m: config.target_height_m,
            })
            .collect::<Vec<_>>();
        match map.place_points_on_planes(
            &motion.generalized,
            &targets,
            &config.bounds,
            &config.placement,
        ) {
            Ok(fit) => {
                fits.push(json!({"iterations":fit.iterations,"maximum_plane_error_m":fit.maximum_plane_error_m,
                    "minimum_scaled_singular_value":fit.motion.minimum_scaled_singular_value,
                    "maximum_scaled_closure_error":fit.motion.maximum_scaled_position_error}));
                motion = fit.motion;
                let points = lowest_points(art, &motion.generalized, &links)?;
                let poses = art.poses(&motion.generalized);
                completed = points.iter().all(|p| {
                    let (r, x) = &poses[p.link];
                    ((x + r * nalgebra::Vector3::from(p.local_point_m)).z - config.target_height_m)
                        .abs()
                        <= config.placement.tolerance_m
                });
                if completed {
                    break;
                }
            }
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
    }
    if !completed && failure.is_none() {
        failure = Some("support vertex reselection limit".into());
    }
    println!("{}", serde_json::to_string_pretty(&json!({"version":1,"completed":completed,"failure":failure,
        "source":session.scene.robot.source,"scene_options":session.scene.options,"config":config,
        "before":before,"after":report(&motion.generalized)?,"fits":fits,
        "original_closure":art.original_closure(&motion.generalized),
        "notes":["Geometric initializer only; no force equilibrium or dynamic trajectory is established.",
            "Height uses the same sampled contact vertices as the runtime floor model; this is not a full CAD surface-clearance certificate.",
            "Bounds are experiment inputs, not validated hardware travel limits. Base pose and authored geometry/mass remain unchanged.",
            "Motor internal states and controller history must be initialized consistently before running this pose dynamically."]
    })).map_err(|e| e.to_string())?);
    Ok(completed)
}
fn main() {
    match run() {
        Ok(true) => (),
        Ok(false) => std::process::exit(2),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
