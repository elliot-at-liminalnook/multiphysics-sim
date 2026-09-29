use super::*;
use sim_inspect::{
    animation::{AnimationDescription, scalar},
    live::{LiveSnapshot, native::Subscriber},
};
use std::sync::Arc;
#[derive(Default)]
pub(super) struct LivePresentation {
    subscriber: Option<Subscriber>,
    revision: u64,
    pub snapshot: Option<Arc<LiveSnapshot>>,
    pub error: Option<String>,
}
#[derive(Component)]
pub(super) struct LiveReadouts;
impl SpatialScene {
    pub fn set_animation(
        &mut self,
        animation: AnimationDescription,
    ) -> Result<(), InspectionError> {
        animation.validate(&self.description, &self.spatial)?;
        self.animation = Some(animation);
        Ok(())
    }
    pub fn connect_live(&mut self, directory: std::path::PathBuf) {
        self.live.subscriber = Some(Subscriber::new(
            Arc::new(self.description.clone()),
            directory,
        ));
    }
    pub(super) fn frame(&self) -> Option<&sim_inspect::SampleFrame> {
        self.live.snapshot.as_ref()?.frame.as_ref()
    }
    pub(super) fn live_status(&self) -> String {
        if self.animation.is_none() {
            return "Static preview | no live connection".into();
        }
        if let Some(e) = &self.live.error {
            return e.clone();
        }
        let Some(snapshot) = &self.live.snapshot else {
            return "Waiting for simulation measurements".into();
        };
        if let Some(e) = &snapshot.error {
            return format!("Simulation error: {e}");
        }
        snapshot
            .status
            .as_ref()
            .map(|s| {
                format!(
                    "{:?} | {:.3} s\nStep {} | generation {}",
                    s.phase, s.time, s.step, s.generation
                )
            })
            .unwrap_or_else(|| "Building simulation worker".into())
    }
    pub(super) fn live_readouts(&self) -> String {
        let Some(a) = &self.animation else {
            return String::new();
        };
        let mut text = format!("LIVE MEASUREMENTS\n{}\n", self.live_status());
        for r in &a.readouts {
            let o = &self.description.observables[&r.observable];
            text.push_str(&format!("\n{}\n", r.label));
            if let Some(v) = scalar(self.frame(), &r.observable) {
                text.push_str(&format!(
                    "{:.4} {}\n{} at {:.3} s\n",
                    v.value,
                    sim_inspect::plot::unit(&self.description, o),
                    if v.accepted_stage {
                        "Accepted stage"
                    } else {
                        "Sample"
                    },
                    v.time
                ));
            } else {
                text.push_str("Unavailable for this sample\n");
            }
            if let Some(sign) = &o.sign_convention {
                text.push_str(&format!("{sign}\n"));
            }
        }
        let scales: std::collections::BTreeSet<_> = a
            .colors
            .iter()
            .map(|c| format!("{:.2} to {:.2} K", c.range_kelvin[0], c.range_kelvin[1]))
            .collect();
        for scale in scales {
            text.push_str(&format!("\nTemperature color\n{scale}\n"));
        }
        text.push_str("Teal to orange. Clamped display scale, not a safety limit. Uniform temperature, not a field.\n\nWhite spoke: sampled shaft orientation. Faster than 45° per drawn frame it becomes a motion-blur fan over the angle swept.\n");
        text
    }
}
pub(super) fn sync_live(mut scene: ResMut<SpatialScene>) {
    scene.poll_live();
}
impl SpatialScene {
    pub(super) fn poll_live(&mut self) {
        let scene = self;
        let Some(subscriber) = &scene.live.subscriber else {
            return;
        };
        let received = subscriber.latest();
        if received.revision != scene.live.revision || received.error != scene.live.error {
            scene.live.revision = received.revision;
            scene.live.snapshot = received.snapshot;
            scene.live.error = received.error;
        }
    }
}
/// A pure mapping from a source pose and a sample, independent of render cadence.
pub(super) fn part_transform(scene: &SpatialScene, index: usize) -> Transform {
    part_transform_at(scene, index, scene.frame())
}
/// A part's pose for a given sample frame (the live one, or a companion run's).
pub(super) fn part_transform_at(scene: &SpatialScene, index: usize, frame: Option<&sim_inspect::SampleFrame>) -> Transform {
    let p = &scene.spatial.parts[index];
    let mut translation = Vec3::from_array(p.position);
    let mut rotation = Quat::from_array(p.rotation_xyzw);
    if let Some(binding) = scene
        .animation
        .as_ref()
        .and_then(|a| a.rotations.iter().find(|r| r.part == p.id))
    {
        if let Some(v) = scalar(frame, &binding.observable) {
            let q = Quat::from_axis_angle(
                Vec3::from_array(binding.axis),
                v.value.rem_euclid(std::f64::consts::TAU) as f32,
            );
            let pivot = Vec3::from_array(binding.pivot);
            translation = pivot + q * (translation - pivot);
            rotation = q * rotation;
        }
    }
    if let Some(binding) = scene.animation.as_ref().and_then(|a| a.translations.iter().find(|t| t.part == p.id)) {
        if let Some(v) = scalar(frame, &binding.observable) {
            translation += Vec3::from_array(binding.axis) * (v.value - binding.reference_m) as f32 * scene.motion_scale;
        }
    }
    if scene.explode_t > 0. {
        translation += explode_offset(scene, index) * scene.explode_t;
    }
    Transform::from_translation(translation).with_rotation(rotation)
}
/// Where a part goes in the exploded view: its authored offset, or (when
/// none was authored) outward from the assembly's centre.
pub(super) fn explode_offset(scene: &SpatialScene, index: usize) -> Vec3 {
    let p = &scene.spatial.parts[index];
    let authored = Vec3::from_array(p.exploded_offset);
    if authored != Vec3::ZERO {
        return authored;
    }
    // No authored offset: move away from the middle of the part's own
    // assembly by at most about two part sizes. The middle is the median
    // position, so a far outlier (a load on a long rope) neither shifts the
    // rest of the machine nor flings it out of view.
    let parent = p.component.rsplit_once('/').map(|(parent, _)| parent);
    let inside = |c: &str| parent.is_none_or(|q| c.starts_with(&format!("{q}/")));
    let positions: Vec<Vec3> = scene.spatial.parts.iter().filter(|q| inside(&q.component)).map(|q| Vec3::from_array(q.position)).collect();
    let median = |axis: usize| {
        let mut v: Vec<f32> = positions.iter().map(|q| q[axis]).collect();
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(0.)
    };
    let middle = Vec3::new(median(0), median(1), median(2));
    let (_, size) = scene.bounds_of(Some(&p.component));
    let away = Vec3::from_array(p.position) - middle;
    away.normalize_or_zero() * (away.length() * 0.8).min(size * 2.)
}
/// The part's bound temperature (K), when it has one and a sample.
pub(super) fn part_temperature(scene: &SpatialScene, index: usize) -> Option<f64> {
    let p = &scene.spatial.parts[index];
    let binding = scene.animation.as_ref()?.colors.iter().find(|c| c.part == p.id)?;
    scalar(scene.frame(), &binding.observable).map(|v| v.value)
}
pub(super) fn part_color(scene: &SpatialScene, index: usize) -> Option<[f32; 3]> {
    let p = &scene.spatial.parts[index];
    let binding = scene
        .animation
        .as_ref()?
        .colors
        .iter()
        .find(|c| c.part == p.id)?;
    // Neutral gray communicates that a bound temperature has no usable sample.
    Some(
        scalar(scene.frame(), &binding.observable)
            .map(|v| binding.color(v.value))
            .unwrap_or([0.42, 0.44, 0.47]),
    )
}
/// Spoke markers on spinning parts. A spoke that turns more than
/// [`BLUR_STEP`] between drawn frames would alias (the wagon-wheel effect:
/// it looks slow, still or reversed), so the angle swept since the last
/// frame is drawn as a fading motion-blur fan instead, a full disc beyond
/// one turn. Display only; the sampled angle is unchanged.
pub(super) fn draw_markers(scene: Res<SpatialScene>, mut gizmos: Gizmos, mut last: Local<std::collections::HashMap<String, f64>>) {
    let Some(a) = &scene.animation else { return };
    for r in &a.rotations {
        let Some(radius) = r.marker_radius else {
            continue;
        };
        let Some((index, p)) = scene
            .spatial
            .parts
            .iter()
            .enumerate()
            .find(|(_, p)| p.id == r.part)
        else {
            continue;
        };
        let Some(angle) = scalar(scene.frame(), &r.observable).map(|v| v.value) else {
            continue;
        };
        let swept = angle - last.insert(r.part.clone(), angle).unwrap_or(angle);
        if scene.state.hidden.contains(&p.component) {
            continue;
        }
        let transform = part_transform(&scene, index);
        let half = match p.shape {
            SpatialShape::Cylinder { length, .. } => length * 0.5 + 0.001,
            _ => 0.001,
        };
        for side in [-1., 1.] {
            let center = transform.transform_point(Vec3::Y * half * side);
            let spoke = |turn: f32| transform.transform_point(Quat::from_rotation_y(turn) * Vec3::new(radius, half * side, 0.));
            if swept.abs() <= BLUR_STEP || scene.reduced_motion {
                let end = spoke(0.);
                gizmos.line(center, end, Color::WHITE);
                gizmos.sphere(Isometry3d::from_translation(end), radius * 0.09, Color::WHITE);
                continue;
            }
            let span = swept.abs().min(std::f64::consts::TAU) as f32 * swept.signum() as f32;
            if scene.state.strobe {
                // Stroboscope: a few crisp spokes, as if lit by evenly spaced flashes.
                for k in 0..STROBE_FLASHES {
                    let f = k as f32 / STROBE_FLASHES as f32;
                    gizmos.line(center, spoke(-span * f), Color::srgba(1., 1., 1., 0.9));
                }
                continue;
            }
            for k in 0..BLUR_SPOKES {
                let f = k as f32 / BLUR_SPOKES as f32;
                gizmos.line(center, spoke(-span * f), Color::srgba(1., 1., 1., 0.75 * (1. - f)));
            }
        }
    }
}
/// Largest spoke turn between drawn frames shown as a single spoke (rad).
const BLUR_STEP: f64 = std::f64::consts::FRAC_PI_4;
const BLUR_SPOKES: usize = 16;
const STROBE_FLASHES: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;
    use sim_inspect::{
        Availability, SampleFrame, SampleValue,
        live::{Phase, SessionStatus},
    };
    fn scene() -> SpatialScene {
        let mut scene = crate::tests::fixture();
        scene
            .set_animation(
                serde_json::from_str(include_str!(
                    "../../../examples/systems-viewer/spatial/motor-thermal.animation.json"
                ))
                .unwrap(),
            )
            .unwrap();
        scene
    }
    fn sample(scene: &mut SpatialScene, angle: f64, temperature: f64, generation: u64) {
        let a = scene.animation.as_ref().unwrap();
        let mut d = scene.description.clone();
        for o in d.observables.values_mut() {
            o.availability = Availability::Available;
        }
        d.seal().unwrap();
        let f = SampleFrame {
            version: sim_inspect::SAMPLE_FRAME_VERSION,
            description_id: d.id.clone(),
            model_revision: d.model_revision,
            run_id: "test".into(),
            generation,
            sequence: 1,
            step: 1,
            time: 1.,
            values: BTreeMap::from([
                (
                    a.rotations[0].observable.clone(),
                    SampleValue::Committed {
                        value: angle,
                        sample_time: 1.,
                    },
                ),
                (
                    a.colors[0].observable.clone(),
                    SampleValue::Committed {
                        value: temperature,
                        sample_time: 1.,
                    },
                ),
            ]),
        };
        scene.live.snapshot = Some(Arc::new(LiveSnapshot {
            version: 1,
            source_description_id: scene.description.id.clone(),
            description: Some(d),
            status: Some(SessionStatus {
                phase: Phase::Paused,
                run_id: "test".into(),
                generation,
                step: 1,
                time: 1.,
                sequence: 1,
                step_wall_seconds: 0.,
                events: 0,
                message: None,
            }),
            frame: Some(f),
            error: None,
        }));
    }
    #[test]
    fn quarter_turn_uses_world_axis_keeps_pivot_and_holds_without_clock_integration() {
        let mut s = scene();
        let geometry = serde_json::to_vec(&s.spatial).unwrap();
        let index = s
            .spatial
            .parts
            .iter()
            .position(|p| p.id == "rotor-wheel")
            .unwrap();
        let initial = part_transform(&s, index);
        sample(&mut s, std::f64::consts::FRAC_PI_2, 295.15, 0);
        let rotated = part_transform(&s, index);
        assert!(rotated.translation.abs_diff_eq(initial.translation, 1e-6));
        assert!((rotated.rotation * Vec3::X).abs_diff_eq(Vec3::NEG_Z, 1e-5));
        for _ in 0..100 {
            assert_eq!(part_transform(&s, index), rotated);
        }
        s.state.exploded = true;
        s.explode_t = 1.;
        assert!(
            (part_transform(&s, index).translation - rotated.translation).abs_diff_eq(
                Vec3::from_array(s.spatial.parts[index].exploded_offset),
                1e-6
            )
        );
        sample(&mut s, 0., 293.15, 1);
        s.state.exploded = false;
        s.explode_t = 0.;
        assert!(
            part_transform(&s, index)
                .rotation
                .abs_diff_eq(initial.rotation, 1e-6)
        );
        assert_eq!(serde_json::to_vec(&s.spatial).unwrap(), geometry);
    }
    #[test]
    fn ecs_applies_pose_and_temperature_without_mutating_physical_definition() {
        let mut s = scene();
        let source = serde_json::to_vec(&s.description).unwrap();
        sample(&mut s, 1., 295.15, 0);
        let motor = s
            .spatial
            .parts
            .iter()
            .position(|p| p.id == "motor")
            .unwrap();
        let wheel = s
            .spatial
            .parts
            .iter()
            .position(|p| p.id == "rotor-wheel")
            .unwrap();
        let expected = part_transform(&s, wheel);
        let color = s.animation.as_ref().unwrap().colors[0].color(295.15);
        let mut app = App::new();
        let mut materials = Assets::<StandardMaterial>::default();
        let handle = materials.add(StandardMaterial::default());
        app.insert_resource(s)
            .insert_resource(materials)
            .add_systems(Update, update_parts);
        let entity = app
            .world_mut()
            .spawn((
                Part { index: wheel },
                Transform::default(),
                Visibility::Inherited,
                MeshMaterial3d(handle.clone()),
            ))
            .id();
        app.update();
        assert_eq!(*app.world().get::<Transform>(entity).unwrap(), expected);
        app.world_mut().get_mut::<Part>(entity).unwrap().index = motor;
        app.world_mut()
            .resource_mut::<SpatialScene>()
            .state
            .selected = None;
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&handle)
                .unwrap()
                .base_color,
            Color::srgb(color[0], color[1], color[2])
        );
        assert_eq!(
            serde_json::to_vec(&app.world().resource::<SpatialScene>().description).unwrap(),
            source
        );
        app.world_mut().resource_mut::<SpatialScene>().live.snapshot = None;
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&handle)
                .unwrap()
                .base_color,
            Color::srgb(0.42, 0.44, 0.47)
        );
    }

    /// Without authored offsets, exploding separates parts by about their own
    /// size; one far-away part does not throw the others out of view.
    #[test]
    fn explode_fallback_stays_near_each_part_despite_an_outlier() {
        let mut s = scene();
        for p in &mut s.spatial.parts {
            p.exploded_offset = [0.; 3];
        }
        let far = s.spatial.parts.len() - 1;
        s.spatial.parts[far].position[1] -= 50.;
        for i in 0..far {
            let (_, size) = s.bounds_of(Some(&s.spatial.parts[i].component.clone()));
            let moved = explode_offset(&s, i).length();
            assert!(moved <= size * 2. + 1e-6, "part {i} moved {moved}");
            assert!(moved < 1., "part {i} thrown {moved} m by the outlier");
        }
    }
}
