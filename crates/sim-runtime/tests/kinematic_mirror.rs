//! The suspended calibration mirror poses the CAD quadruped from motor coordinates.
use sim_runtime::{kinematic_mirror::KinematicMirror, session::Scene};

const SCENE: &str = "../../examples/full-robot/measured-actuator-integration/browser-control-400hz/scene.json";

fn position(pose: &sim_runtime::kinematic_mirror::MirrorPose, link: &str) -> [f64; 3] {
    pose.poses.iter().find(|p| p.name == link).unwrap().position_m
}

#[test]
fn each_leg_servo_moves_only_its_own_leg_and_the_base_stays_suspended() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SCENE);
    let scene: Scene = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut mirror = KinematicMirror::new(scene, 0.2).unwrap();
    let joints = mirror.motor_joints();
    assert_eq!(joints.len(), 12);
    let home: Vec<f64> = mirror.coordinates().iter().map(|c| c.home).collect();
    let rest = mirror.pose(&home).unwrap();
    assert!(rest.maximum_scaled_closure_error < 1e-6);
    let chassis = position(&rest, "Robot | Chassis and hip mounts");
    for (i, joint) in joints.iter().enumerate().filter(|(_, j)| j.starts_with("+X |")) {
        let mut q = home.clone();
        q[i] += 0.05;
        let moved = mirror.pose(&q).unwrap();
        assert!(moved.maximum_scaled_closure_error < 1e-6, "{joint}");
        assert_eq!(position(&moved, "Robot | Chassis and hip mounts"), chassis, "base held for {joint}");
        let delta = |link: &str| {
            let (a, b) = (position(&rest, link), position(&moved, link));
            (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt()
        };
        let own = ["+X | Sliding foot crosshead", "+X | Thigh sector gear"].iter().map(|l| delta(l)).fold(0.0, f64::max);
        let other = ["-X | Sliding foot crosshead", "+Y | Sliding foot crosshead", "-Y | Sliding foot crosshead"].iter().map(|l| delta(l)).fold(0.0, f64::max);
        eprintln!("{joint}: own leg moved {own:.5} m, other legs {other:.2e} m");
        assert!(own > 1e-4, "{joint} must move its leg");
        assert!(other < 1e-9, "{joint} must not move other legs");
    }
    // Returning to home reproduces the rest pose on the same assembly branch.
    let back = mirror.pose(&home).unwrap();
    assert!((position(&back, "+X | Sliding foot crosshead")[2] - position(&rest, "+X | Sliding foot crosshead")[2]).abs() < 1e-9);
}

/// The printed knee cannot reach its CAD home (fully extended), so the leg is
/// aligned at mid-travel: halfway between the knee joint's CAD limits. That
/// pose must solve with the linkage closed and within the authored limits.
#[test]
fn knee_mid_travel_alignment_pose_solves_inside_its_limits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SCENE);
    let scene: Scene = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut mirror = KinematicMirror::new(scene, 0.2).unwrap();
    let coordinates = mirror.coordinates();
    let i = coordinates.iter().position(|c| c.joint == "+X | Foot servo output").unwrap();
    let (lo, hi) = (coordinates[i].lower.unwrap(), coordinates[i].upper.unwrap());
    let mid = (lo + hi) / 2.;
    assert!(mid < coordinates[i].home - 1.0, "mid-travel {mid} rad is well away from CAD home {}", coordinates[i].home);
    let mut q: Vec<f64> = coordinates.iter().map(|c| c.home).collect();
    q[i] = mid;
    let pose = mirror.pose(&q).unwrap();
    assert!(pose.maximum_scaled_closure_error < 1e-6);
    assert!(pose.authored_limit_violations.iter().all(|n| !n.starts_with("+X |")), "{:?}", pose.authored_limit_violations);
    let home = mirror.pose(&coordinates.iter().map(|c| c.home).collect::<Vec<_>>()).unwrap();
    let foot = |p: &sim_runtime::kinematic_mirror::MirrorPose| position(p, "+X | Sliding foot crosshead");
    let lift = foot(&pose)[2] - foot(&home)[2];
    eprintln!("knee mid-travel {mid:.3} rad ({:.1} deg): foot {lift:.4} m above its CAD-home height", mid.to_degrees());
    // Measured by stepping the knee from home: mid-travel raises the crosshead
    // about 8 cm. A single solve from home landed on the other branch (12 cm lower).
    assert!(lift > 0.05, "mid-travel raises the foot on the CAD assembly branch");
}
