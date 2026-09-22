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
        text.push_str("Teal to orange. Clamped display scale, not a safety limit. Uniform temperature, not a field.\n\nWhite spoke: sampled shaft orientation. No extrapolation.\n");
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
    let p = &scene.spatial.parts[index];
    let mut translation = Vec3::from_array(p.position);
    let mut rotation = Quat::from_array(p.rotation_xyzw);
    if let Some(binding) = scene
        .animation
        .as_ref()
        .and_then(|a| a.rotations.iter().find(|r| r.part == p.id))
    {
        if let Some(v) = scalar(scene.frame(), &binding.observable) {
            let q = Quat::from_axis_angle(
                Vec3::from_array(binding.axis),
                v.value.rem_euclid(std::f64::consts::TAU) as f32,
            );
            let pivot = Vec3::from_array(binding.pivot);
            translation = pivot + q * (translation - pivot);
            rotation = q * rotation;
        }
    }
    if scene.state.exploded {
        translation += Vec3::from_array(p.exploded_offset);
    }
    Transform::from_translation(translation).with_rotation(rotation)
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
pub(super) fn draw_markers(scene: Res<SpatialScene>, mut gizmos: Gizmos) {
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
        if scene.state.hidden.contains(&p.component)
            || scalar(scene.frame(), &r.observable).is_none()
        {
            continue;
        }
        let transform = part_transform(&scene, index);
        let half = match p.shape {
            SpatialShape::Cylinder { length, .. } => length * 0.5 + 0.001,
            _ => 0.001,
        };
        for side in [-1., 1.] {
            let center = transform.transform_point(Vec3::Y * half * side);
            let end = transform.transform_point(Vec3::new(radius, half * side, 0.));
            gizmos.line(center, end, Color::WHITE);
            gizmos.sphere(
                Isometry3d::from_translation(end),
                radius * 0.09,
                Color::WHITE,
            );
        }
    }
}

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
        assert!(
            (part_transform(&s, index).translation - rotated.translation).abs_diff_eq(
                Vec3::from_array(s.spatial.parts[index].exploded_offset),
                1e-6
            )
        );
        sample(&mut s, 0., 293.15, 1);
        s.state.exploded = false;
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
}
