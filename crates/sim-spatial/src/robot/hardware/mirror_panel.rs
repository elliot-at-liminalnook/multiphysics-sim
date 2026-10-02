//! The "Simulated leg mirror" section and the mirror's frame systems
//! (calibration-mirror.mjs:24-29 for the section, calibration-ui.mjs:121-122
//! and :241-270 for when it begins, updates and samples a gait).
//!
//! - `mirror_sync` (SimSync, before `robot::apply_frames`): takes the
//!   mirror worker's results, builds the roles from the first status that
//!   lists axes, begins or ends the mirror as the panel opens, closes, the
//!   link comes or goes and the preference changes, judges the link's health
//!   every frame (`LinkSnapshot::health`: a stale or lost link holds the
//!   last pose and says so; becoming live again forces an update), updates
//!   it on every new link snapshot, follows a gait played in Sim or Both
//!   (Both on the leg's clock, `LinkSnapshot::leg_clock`), and writes
//!   `RobotView::mirror` (poses and tinted links) with `pose_dirty`.
//! - `mirror_panel` (Present): fills [`MirrorRoot`] (spawned by the panel at
//!   the page's position) with the section's controls, rebuilt only when the
//!   roles or preferences change; the status line is updated in place.
use super::actions::HardwareAction;
use super::link::LinkHealth;
use super::{Hardware, MirrorDisplay, mirror};
use crate::app::actions::Act;
use crate::app::{ViewerMode, ViewerSet};
use crate::robot::{RobotAction, RobotView};
use crate::robot::run::{Phase, RunAction};
use crate::ui_kit::{Kit, SUBTLE, TEXT, UiFonts, size, wrap};
use bevy::prelude::*;
use std::time::Instant;

/// The section's content node (the panel spawns it inside "Simulated leg mirror").
#[derive(Component)]
pub(crate) struct MirrorRoot;

/// The status line (`#mirror-status`).
#[derive(Component)]
struct MirrorStatus;

pub(crate) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (mirror_sync.in_set(ViewerSet::SimSync).before(crate::robot::RobotSet::Frames), mirror_panel.in_set(ViewerSet::Present)).run_if(in_state(ViewerMode::Robot)),
    );
}

/// What `mirror_sync` last saw.
#[derive(Default)]
struct Seen {
    /// The link snapshot's (generation, revision).
    snapshot: Option<(u64, u64)>,
    /// The loaded run's identity (None: none, or `--robot FILE`).
    scene: Option<Option<mirror::SceneId>>,
    wanted: bool,
}

#[allow(clippy::too_many_arguments)]
fn mirror_sync(hw: Option<ResMut<Hardware>>, view: Option<ResMut<RobotView>>, mut acts: MessageWriter<Act<RobotAction>>, mut redraw: MessageWriter<bevy::window::RequestRedraw>, mut seen: Local<Seen>) {
    let (Some(mut hw), Some(mut view)) = (hw, view) else { return };
    let (hw, view) = (&mut *hw, &mut *view);
    // The leg's data now, every frame and before anything can pose from it (a
    // gait sample landing in `poll` re-poses the encoders): a link that stops
    // answering goes stale without a new revision. No link: nothing read.
    let now = Instant::now();
    let health = if hw.link.is_some() { hw.snapshot.health(now) } else { LinkHealth::Waiting };
    let recovered = hw.mirror.set_leg_health(&health);
    // The worker's results first: loaded coordinates show the mirror this frame.
    if let Some(solved) = hw.mirror.poll() {
        if let Some(display) = view.mirror.as_mut() {
            display.poses = solved.poses;
            view.pose_dirty = true;
        }
    }
    // The link's snapshot (copied once a frame into `Hardware::snapshot`): roles once, the state when it changed.
    if hw.link.is_some() && hw.mirror.roles().is_none() {
        if let Some(doc) = hw.snapshot.state.calibration.as_ref() {
            hw.mirror.set_roles(doc.axes.iter().map(|(id, a)| (*id, a.role.clone())).collect());
            // The page begins the mirror as soon as it is constructed.
            hw.mirror.begin();
        }
    }
    let want = hw.mirror.settings.enabled && hw.open && hw.link.is_some() && hw.mirror.roles().is_some();
    let scene = mirror::scene_of(view);
    let key = scene.as_ref().ok().and_then(|s| s.as_ref().map(mirror::SceneSource::id));
    if seen.scene.as_ref() != Some(&key) {
        // A preset opened, switched or reloaded: pose the new scene.
        seen.scene = Some(key);
        hw.mirror.begin();
    }
    if want && !seen.wanted {
        hw.mirror.begin();
    } else if !want && seen.wanted {
        hw.mirror.end();
    }
    seen.wanted = want;
    let snap = &hw.snapshot;
    let key = (snap.generation, snap.revision);
    if hw.link.is_some() && (recovered || seen.snapshot != Some(key)) {
        seen.snapshot = Some(key);
        // `update` poses from the encoders only while live (else it holds);
        // live again, it is forced so the live pose and line return.
        hw.mirror.update(&snap.state, recovered);
    }
    // No link: no gait run (a dropped link ends its gait).
    let gait_run = hw.link.as_ref().and(snap.gait.as_ref());
    let leg_clock = hw.link.as_ref().and_then(|_| snap.leg_clock(now));
    hw.mirror.follow_gait(gait_run, snap.compiled_gait.as_ref(), leg_clock.as_ref(), now);
    if want && hw.mirror.due() {
        let links = mirror::link_names(view);
        if let Some(tinted) = hw.mirror.prepare(scene, &links) {
            if let Some(display) = view.mirror.as_mut() {
                // Begun again while shown (leg, joint, sign or alignment changed):
                // keep the mirror's pose until the next solve; only the tint changes.
                display.tinted = tinted;
            } else {
                // `robotViewer.begin(links)`: pause, tint, keep the current pose until the first solve, fit.
                let poses = view.run.as_ref().and_then(|r| r.display_poses()).map(<[_]>::to_vec).unwrap_or_default();
                if view.run.as_ref().is_some_and(|r| r.phase() == Phase::Running) {
                    acts.write(Act::quiet(RobotAction::Run { action: RunAction::Pause }));
                }
                view.mirror = Some(MirrorDisplay { poses, tinted });
                acts.write(Act::quiet(RobotAction::Fit));
            }
            view.pose_dirty = true;
        }
    }
    // `robotViewer.end()`: the run's frame shows again, untinted.
    if (hw.mirror.take_ended() || !hw.mirror.shown()) && view.mirror.is_some() {
        view.mirror = None;
        view.pose_dirty = true;
    }
    if hw.mirror.working() {
        redraw.write(bevy::window::RequestRedraw);
    }
}

/// What the section was last built from.
type Built = Option<(Entity, u64, bool)>;

fn mirror_panel(mut commands: Commands, hw: Option<Res<Hardware>>, fonts: Res<UiFonts>, roots: Query<Entity, With<MirrorRoot>>, mut status: Query<&mut Text, With<MirrorStatus>>, mut built: Local<Built>) {
    let (Some(hw), Ok(root)) = (hw, roots.single()) else { return };
    let m = &hw.mirror;
    let key = (root, m.revision, m.roles().is_some());
    if *built != Some(key) {
        *built = Some(key);
        commands.entity(root).despawn_related::<Children>();
        let k = Kit::new(&fonts);
        commands.entity(root).with_children(|p| fill(p, &k, m));
        return;
    }
    let text = m.status_text();
    for mut t in &mut status {
        if t.0 != text {
            t.0 = text.clone();
        }
    }
}

/// The section's controls in the page's order (:24-29).
fn fill(p: &mut ChildSpawnerCommands, k: &Kit, m: &mirror::Mirror) {
    let Some(roles) = m.roles() else {
        // The page builds this section from the first status that lists the motors.
        p.spawn(k.caption("Connect to the calibration server to show the real leg on the simulated robot."));
        return;
    };
    let s = &m.settings;
    p.spawn(wrap()).with_children(|r| {
        r.spawn(k.chip("Show the real leg on the suspended simulated robot", HardwareAction::MirrorEnabled { on: !s.enabled }, s.enabled, true));
    });
    p.spawn(wrap()).with_children(|r| {
        r.spawn(k.text("Simulated leg", size::SMALL, SUBTLE, 0));
        r.spawn(k.segments()).with_children(|seg| {
            for leg in mirror::LEGS {
                seg.spawn(k.segment(leg, HardwareAction::MirrorLeg { leg: leg.into() }, s.leg == leg, true));
            }
        });
    });
    for (id, role) in roles {
        let Some(b) = s.bindings.get(id) else { continue };
        let id = *id;
        p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), margin: UiRect::top(Val::Px(4.0)), flex_shrink: 0.0, ..default() }).with_children(|row| {
            row.spawn(k.text(format!("{role}  ID {id}"), size::SMALL, TEXT, 1));
            row.spawn(k.segments()).with_children(|seg| {
                for (joint, label) in mirror::JOINTS {
                    seg.spawn(k.segment(label, HardwareAction::MirrorJoint { id, joint: joint.into() }, b.joint == joint, true));
                }
            });
            row.spawn(wrap()).with_children(|r| {
                r.spawn(k.segments()).with_children(|seg| {
                    for (polarity, label) in [(1i8, "+"), (-1, "−")] {
                        seg.spawn(k.segment(label, HardwareAction::MirrorPolarity { id, polarity }, b.polarity == polarity, true));
                    }
                });
                r.spawn(k.segments()).with_children(|seg| {
                    for align in [super::actions::Align::Home, super::actions::Align::Mid] {
                        seg.spawn(k.segment(mirror::align_label(align), HardwareAction::MirrorAlign { id, align }, b.align == align, true));
                    }
                });
            });
        });
    }
    p.spawn(k.note(mirror::NOTE));
    p.spawn((k.caption(m.status_text()), MirrorStatus));
}
