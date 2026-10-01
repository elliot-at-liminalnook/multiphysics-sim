//! The "hardware-mirror" worker thread (the page's module worker).
use super::{Coordinate, LIFT_M, MirrorCommand, MirrorShared, Solved};
use sim_runtime::gait_playback::{Gait, GovernedGait};
use sim_runtime::kinematic_mirror::KinematicMirror;
use std::sync::{Arc, Mutex, MutexGuard, mpsc};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The worker (the page's module worker: `mirror_load`, `mirror_pose`,
/// `gait_load`, `gait_sample`; web/worker.js:68-91).
pub(super) fn worker(rx: mpsc::Receiver<MirrorCommand>, out: Arc<Mutex<MirrorShared>>) {
    let mut mirror: Option<KinematicMirror> = None;
    let mut links: Vec<String> = Vec::new();
    let mut gait: Option<(Gait, GovernedGait)> = None;
    loop {
        let Ok(first) = rx.recv() else { return };
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        // Latest wins: only the newest pose request of the batch is solved.
        let newest_pose = batch.iter().rposition(|c| matches!(c, MirrorCommand::Pose { .. }));
        for (i, command) in batch.into_iter().enumerate() {
            match command {
                MirrorCommand::Load { scene, links: names } => {
                    links = names;
                    let result = KinematicMirror::new(scene.scene().clone(), LIFT_M).map(|m| {
                        let c = m.coordinates().into_iter().map(|c| Coordinate { joint: c.joint, home: c.home, lower: c.lower, upper: c.upper }).collect();
                        mirror = Some(m);
                        c
                    });
                    lock(&out).coordinates = Some(result);
                }
                MirrorCommand::Pose { .. } if Some(i) != newest_pose => {}
                MirrorCommand::Pose { seq, values } => {
                    let result = match mirror.as_mut() {
                        None => Err("load the kinematic mirror first".to_string()),
                        Some(m) => m.pose(&values).map(|pose| {
                            let (poses, _) = crate::robot::gait::map_poses(&pose.poses, &links);
                            (Solved { poses }, pose.authored_limit_violations)
                        }),
                    };
                    lock(&out).pose = Some((seq, result));
                }
                MirrorCommand::Gait { number, compiled, name } => {
                    let result = Gait::from_compiled(&compiled, &name).map(|g| {
                        gait = Some((g.clone(), GovernedGait::new(g)));
                    });
                    if result.is_err() {
                        gait = None;
                    }
                    lock(&out).gait = Some((number, result));
                }
                MirrorCommand::Sample { seq, t, dt, scale, reset } => {
                    let result = match gait.as_mut() {
                        None => Err("load a gait first".to_string()),
                        Some((g, governed)) => {
                            if reset {
                                *governed = GovernedGait::new(g.clone());
                            }
                            // Governed (as the simulation commands it) when the gait has a governor.
                            let values = if g.info.governor.is_some() { governed.step(t, dt, scale).map(|v| v.into_iter().map(|(q, _)| q).collect()) } else { g.sample(t) };
                            values.map(|v: Vec<f64>| g.info.joints.iter().cloned().zip(v).collect())
                        }
                    };
                    lock(&out).sample = Some((seq, result));
                }
            }
        }
    }
}
