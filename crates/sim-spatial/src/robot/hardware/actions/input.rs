//! Input mappings (`ViewerSet::Input`): the panel's buttons, hold-to-move
//! jog buttons, keys, window focus loss and sliders, each written as an
//! `Act<HardwareAction>` for [`super::apply`].
use super::{Direction, HardwareAction, Loss};
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::robot::hardware::Hardware;
use crate::robot::hardware::panel::{JogButton, PanelSlider};
use bevy::prelude::*;

/// Input: a pressed panel button's action (not while disabled). The jog
/// buttons are hold-to-move ([`jog_buttons`]).
pub(super) fn buttons(clicks: Query<(&HardwareAction, Option<&Enabled>), (With<crate::ui_kit::activation::Activated>, Without<JogButton>)>, mut out: MessageWriter<Act<HardwareAction>>) {
    for (action, enabled) in &clicks {
        if enabled.is_none_or(|e| e.0) {
            out.write(Act::ui(action.clone()));
        }
    }
}

/// Input: "Upper ↑"/"Lower ↓" pressed is `JogPress`; the press ending
/// (released anywhere: `bevy::ui` keeps `Pressed` until the left button is
/// released, like the page's pointer capture) is `JogRelease`.
pub(super) fn jog_buttons(keys: Res<ButtonInput<KeyCode>>, mut jogs: Query<(&Interaction, &mut JogButton, Option<&Enabled>)>, mut out: MessageWriter<Act<HardwareAction>>) {
    for (interaction, mut jog, enabled) in &mut jogs {
        let pressed = *interaction == Interaction::Pressed;
        if pressed && !jog.held && enabled.is_none_or(|e| e.0) {
            jog.held = true;
            out.write(Act::ui(HardwareAction::JogPress { direction: jog.direction }));
        } else if !pressed && jog.held {
            jog.held = false;
            let code = match jog.direction { Direction::Upper => KeyCode::KeyQ, Direction::Lower => KeyCode::KeyA };
            if !keys.pressed(code) { out.write(Act::ui(HardwareAction::JogRelease { direction: jog.direction })); }
        }
    }
}

/// Input: the page's keys (:290-296) while the panel is shown and no
/// Ctrl/Cmd/Alt is held: Z or Escape is STOP; Q/A press is `JogPress`
/// (a key repeat is not a press in Bevy). A Q/A release is `JogRelease`
/// whether or not the panel is shown (the page's keyup); [`apply`](super::apply) ignores
/// a release it did not accept a press for, and holds when both are held.
///
/// A kit text field with the keyboard (`ui_kit::text::Typing`): a Q/A press
/// is not a jog. STOP is not gated here, and a release never is (the kit
/// releases held keys when a field takes the keyboard, so a held jog's
/// release arrives here and stops it). A focused field does consume Z and
/// Escape (as its text and its Cancel), which is why no field may hold the
/// keyboard while the panel is shown: the gait path field refuses and gives
/// it up, and the document picker closes, when the panel opens.
pub(super) fn keys(jogs: Query<&JogButton>, keys: Res<ButtonInput<KeyCode>>, typing: crate::ui_kit::text::Typing, hw: Option<Res<Hardware>>, mut out: MessageWriter<Act<HardwareAction>>) {
    const JOG: [(KeyCode, Direction); 2] = [(KeyCode::KeyQ, Direction::Upper), (KeyCode::KeyA, Direction::Lower)];
    let Some(hw) = hw else { return };
    let modified = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::AltLeft, KeyCode::AltRight]);
    if hw.open && !modified {
        if keys.any_just_pressed([KeyCode::KeyZ, KeyCode::Escape]) {
            out.write(Act::ui(HardwareAction::Stop));
        }
        for (code, direction) in JOG {
            if keys.just_pressed(code) && !typing.get() {
                out.write(Act::ui(HardwareAction::JogPress { direction }));
            }
        }
    }
    for (code, direction) in JOG {
        if keys.just_released(code) && !jogs.iter().any(|jog| jog.direction == direction && jog.held) {
            out.write(Act::ui(HardwareAction::JogRelease { direction }));
        }
    }
}

/// Input: the window losing focus stops drive (the page's
/// `visibilitychange`), a close request too (`pagehide`; the window is
/// despawned a frame later, so this frame's apply still runs). The close is
/// written `Origin::Quiet`, the only origin whose `Loss::Leaving` also
/// writes STOP synchronously (`handlers::loss`).
pub(super) fn window_loss(mut focus: MessageReader<bevy::window::WindowFocused>, mut close: MessageReader<bevy::window::WindowCloseRequested>, hw: Option<Res<Hardware>>, mut out: MessageWriter<Act<HardwareAction>>) {
    let lost = focus.read().filter(|e| !e.focused).count() > 0;
    let closing = close.read().count() > 0;
    if hw.is_none() {
        return;
    }
    if closing {
        out.write(Act::quiet(HardwareAction::Loss { reason: Loss::Leaving }));
    } else if lost {
        out.write(Act::quiet(HardwareAction::Loss { reason: Loss::FocusLost }));
    }
}

/// What the slider input last held.
#[derive(Default)]
pub(super) struct HeldSlider {
    which: Option<PanelSlider>,
    /// A target was sent during this hold (its release is the page's `change`).
    moved_target: bool,
}

/// Input: the panel's sliders while held (the page's `input` events, in
/// each input's steps); releasing the target slider after moving it is
/// `TargetCommit` (its `change`).
pub(super) fn sliders(sliders: Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction, Has<bevy::ui::InteractionDisabled>, &PanelSlider)>, hw: Option<Res<Hardware>>, mut held: Local<HeldSlider>, mut out: MessageWriter<Act<HardwareAction>>) {
    let Some(hw) = hw else {
        *held = HeldSlider::default();
        return;
    };
    let f = &hw.form;
    let mut now = None;
    for (value, pressed, interaction, disabled, which) in &sliders {
        if disabled || !crate::ui_kit::slider_held(pressed, interaction) {
            continue;
        }
        now = Some(*which);
        let x = value.0.clamp(0.0, 1.0) as f64;
        let action = match which {
            PanelSlider::Speed => Some((x * 100.0).round()).filter(|v| *v != f.inputs.speed_percent).map(|percent| HardwareAction::Speed { percent }),
            PanelSlider::Target => Some((x * 1000.0).round() / 10.0).filter(|v| *v != f.target_percent).map(|percent| HardwareAction::Target { percent }),
            PanelSlider::GaitSpeed => Some(5.0 + (x * 95.0).round()).filter(|v| *v != f.inputs.gait_speed_percent).map(|percent| HardwareAction::GaitSpeed { percent }),
            PanelSlider::GaitEffort => Some(10.0 + (x * 90.0).round()).filter(|v| *v != f.inputs.gait_effort_percent).map(|percent| HardwareAction::GaitEffort { percent }),
            PanelSlider::Pwm => Some((x * 1000.0).round() / 10.0).filter(|v| *v != f.inputs.pwm_percent).map(|percent| HardwareAction::PwmCeiling { percent }),
        };
        if let Some(action) = action {
            if matches!(action, HardwareAction::Target { .. }) {
                held.moved_target = true;
            }
            out.write(Act::ui(action));
        }
    }
    if held.which == Some(PanelSlider::Target) && now != Some(PanelSlider::Target) && held.moved_target {
        out.write(Act::ui(HardwareAction::TargetCommit));
    }
    if now != Some(PanelSlider::Target) {
        held.moved_target = false;
    }
    held.which = now;
}

#[cfg(test)]
mod activation_fixtures {
    use super::*;
    use crate::ui_kit::{Kit, UiFonts};

    // Render the real panel fragments, with placeholder font/image handles.
    fn render(mut commands: Commands) {
        let fonts = UiFonts { regular: Handle::default(), italic: Handle::default(), mono: Handle::default(), icons: Default::default(), medium: Handle::default(), semibold: Handle::default() };
        let k = Kit::new(&fonts);
        commands.spawn(Node::default()).with_children(|root| {
            crate::robot::hardware::panel_sections::top_bar(root, &k);
            crate::robot::hardware::panel_sections::body(root, &k, Handle::default(), Handle::default());
        });
    }

    fn fixture() -> App {
        let mut app = App::new();
        crate::app::configure_sets(&mut app);
        app.add_plugins(crate::ui_kit::text::TextEntryPlugin)
            .add_message::<Act<HardwareAction>>()
            .add_systems(Startup, render)
            .add_systems(Update, buttons.in_set(crate::app::InputSet::Window));
        crate::ui_kit::activation::install(&mut app);
        app.update();
        // Run eligibility for the newly spawned real controls.
        app.update();
        app
    }

    /// Written/source-inspected only: captured ordinary STOP reaches the existing
    /// action owner once; a held jog cannot enter generic activation at all.
    #[test]
    fn rendered_hardware_stop_activates_once_and_jog_is_explicitly_excluded() {
        let mut app = fixture();
        let world = app.world_mut();
        let stop = world.query::<(Entity, &HardwareAction, &bevy::ui::prelude::AccessibleLabel)>().iter(world)
            .find_map(|(entity, action, label)| (matches!(action, HardwareAction::Stop) && label.0 == "Z  Stop").then_some(entity)).unwrap();
        let jogs: Vec<Entity> = world.query_filtered::<Entity, With<JogButton>>().iter(world).collect();
        assert_eq!(jogs.len(), 2);
        for jog in &jogs {
            assert!(world.get::<crate::ui_kit::activation::HeldControl>(*jog).is_some());
            assert!(world.get::<bevy::ui_widgets::Button>(*jog).is_none());
            assert!(world.get::<crate::ui_kit::activation::Ordinary>(*jog).is_none());
            world.trigger(bevy::ui_widgets::Activate { entity: *jog });
        }
        // Drive the pinned press observer on the actual rendered STOP control.
        // No release/click is supplied: activation is intentionally on press.
        let window = world.spawn_empty().id();
        let camera = world.spawn_empty().id();
        world.trigger(bevy::picking::events::Pointer::new(
            bevy::picking::pointer::PointerId::Mouse,
            bevy::picking::pointer::Location {
                target: bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window)).normalize(Some(window)).unwrap(),
                position: Vec2::ZERO,
            },
            bevy::picking::events::Press {
                button: bevy::picking::pointer::PointerButton::Primary,
                hit: bevy::picking::backend::HitData::new(camera, 0.0, None, None),
                count: 1,
            },
            stop,
        ));
        app.update();
        let actions: Vec<_> = app.world_mut().resource_mut::<Messages<Act<HardwareAction>>>().drain().map(|a| a.action).collect();
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], HardwareAction::Stop));
        app.update();
        assert_eq!(app.world_mut().resource_mut::<Messages<Act<HardwareAction>>>().drain().count(), 0);
    }

    /// The actual STOP control remains visible to system_ui through UI Button,
    /// but disabled state refuses generic activation before typed conversion.
    #[test]
    fn rendered_disabled_hardware_control_refuses_activation() {
        let mut app = fixture();
        let world = app.world_mut();
        let stop = world.query::<(Entity, &HardwareAction, &bevy::ui::prelude::AccessibleLabel)>().iter(world)
            .find_map(|(entity, action, label)| (matches!(action, HardwareAction::Stop) && label.0 == "Z  Stop").then_some(entity)).unwrap();
        assert!(world.get::<Button>(stop).is_some());
        world.entity_mut(stop).insert(Enabled(false));
        world.trigger(bevy::ui_widgets::Activate { entity: stop });
        app.update();
        assert_eq!(app.world_mut().resource_mut::<Messages<Act<HardwareAction>>>().drain().count(), 0);
    }
}
