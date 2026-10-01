//! The one text entry, windowless: focus, typing, consumption, release,
//! submit and cancel, and the source guard.
use super::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseButtonInput;
use bevy::input::{ButtonState, InputPlugin};

const A: FieldId = FieldId("test.a");
const B: FieldId = FieldId("test.b");

/// Presses of G seen by a mode key map, gated or not.
#[derive(Resource, Default)]
struct Seen {
    gated: usize,
    ungated: usize,
}

fn mode_key_gated(keys: Res<ButtonInput<KeyCode>>, mut seen: ResMut<Seen>) {
    if keys.just_pressed(KeyCode::KeyG) {
        seen.gated += 1;
    }
}
fn mode_key_ungated(keys: Res<ButtonInput<KeyCode>>, mut seen: ResMut<Seen>) {
    if keys.just_pressed(KeyCode::KeyG) {
        seen.ungated += 1;
    }
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((InputPlugin, TextEntryPlugin))
        .init_resource::<Seen>()
        .add_systems(Update, (mode_key_gated.run_if(not(typing)), mode_key_ungated))
        .add_text_field(A, TextField::new("A").select_on_focus())
        .add_text_field(B, TextField::new("B").enter(EnterKey::ShiftNewline));
    app.update();
    app
}

fn press(app: &mut App, code: KeyCode, key: Key) {
    let text = match &key {
        Key::Character(c) => Some(c.clone()),
        _ => None,
    };
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput { key_code: code, logical_key: key.clone(), state, text: text.clone(), repeat: false, window: Entity::PLACEHOLDER });
    }
}

fn hold(app: &mut App, code: KeyCode, state: ButtonState) {
    app.world_mut().write_message(KeyboardInput { key_code: code, logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified), state, text: None, repeat: false, window: Entity::PLACEHOLDER });
}

fn focus(app: &mut App, id: FieldId, text: &str) {
    let text = text.to_string();
    app.world_mut().run_system_once(move |mut f: TextFocus| assert!(f.focus(id, text.clone()))).unwrap();
}

fn draft(app: &mut App, id: FieldId) -> TextDraft {
    app.world_mut().run_system_once(move |f: TextFocus| f.draft(id).cloned().unwrap()).unwrap()
}

fn is_typing(app: &mut App) -> bool {
    app.world_mut().run_system_once(|t: Typing| t.get()).unwrap()
}

fn messages(app: &mut App) -> Vec<FieldMsg> {
    app.world_mut().resource_mut::<Messages<FieldMsg>>().drain().collect()
}

/// Only the focused field receives typed text: two fields never share a keystroke.
#[test]
fn only_the_focused_field_types() {
    let mut app = app();
    focus(&mut app, A, "10");
    assert_eq!(draft(&mut app, A), TextDraft::new("10", true), "A selects on focus");
    press(&mut app, KeyCode::Digit5, Key::Character("5".into()));
    app.update();
    assert_eq!(draft(&mut app, A).text, "5", "typing replaces the selection");
    assert_eq!(draft(&mut app, B).text, "");
    assert_eq!(messages(&mut app), vec![FieldMsg { field: A, event: FieldEvent::Changed(TextDraft::new("5", false)) }]);
    // B takes the keyboard: A is told, and the next key is B's alone.
    focus(&mut app, B, "");
    assert_eq!(messages(&mut app), vec![FieldMsg { field: A, event: FieldEvent::Blur }]);
    press(&mut app, KeyCode::KeyX, Key::Character("x".into()));
    app.update();
    assert_eq!((draft(&mut app, A).text, draft(&mut app, B).text), ("5".to_string(), "x".to_string()));
}

/// A mode key gated on `not(typing)` does not fire while a field has the
/// keyboard, and the key the field used is consumed for ungated readers too.
#[test]
fn mode_keys_are_silent_while_typing() {
    let mut app = app();
    press(&mut app, KeyCode::KeyG, Key::Character("g".into()));
    app.update();
    let seen = app.world().resource::<Seen>();
    assert_eq!((seen.gated, seen.ungated), (1, 1), "no field: G is a mode key");
    focus(&mut app, A, "");
    assert!(is_typing(&mut app));
    press(&mut app, KeyCode::KeyG, Key::Character("g".into()));
    app.update();
    let seen = app.world().resource::<Seen>();
    assert_eq!((seen.gated, seen.ungated), (1, 1), "typed: no mode key sees G");
    assert_eq!(draft(&mut app, A).text, "g");
}

/// A held key is released when a field takes the keyboard (a robot
/// walking on a held W stops); the release reaches readers once.
#[test]
fn held_keys_are_released_on_focus() {
    let mut app = app();
    hold(&mut app, KeyCode::KeyW, ButtonState::Pressed);
    app.update();
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
    focus(&mut app, A, "");
    app.update();
    let keys = app.world().resource::<ButtonInput<KeyCode>>();
    assert!(!keys.pressed(KeyCode::KeyW) && keys.just_released(KeyCode::KeyW), "W is released once");
    app.update();
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_released(KeyCode::KeyW));
}

/// Moved from the document picker (found by reading there: `clear()`
/// dropped a same-frame release, so a robot walking on a held W kept
/// walking): releasing keeps a release that arrived the same frame.
#[test]
fn release_keeps_same_frame_releases() {
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(KeyCode::KeyW);
    input.press(KeyCode::KeyA);
    input.press(KeyCode::KeyS);
    input.clear();
    // This frame: W released, A still held, D freshly pressed, S re-pressed after a release.
    input.release(KeyCode::KeyW);
    input.press(KeyCode::KeyD);
    input.release(KeyCode::KeyS);
    input.press(KeyCode::KeyS);
    release_held(&mut input);
    assert!(input.just_released(KeyCode::KeyW), "a release this frame survives");
    assert!(input.just_released(KeyCode::KeyA), "a held key is released");
    assert!(input.just_released(KeyCode::KeyS), "a re-pressed key keeps its release");
    assert!(!input.pressed(KeyCode::KeyD) && !input.just_pressed(KeyCode::KeyD) && !input.just_released(KeyCode::KeyD), "a fresh press is dropped");
    assert_eq!(input.get_pressed().count(), 0);
    assert_eq!(input.get_just_pressed().count(), 0);
}

/// Enter submits (the field keeps the keyboard for the owner to accept);
/// Escape cancels and takes the keyboard away; neither reaches a mode key.
#[test]
fn enter_submits_and_escape_cancels() {
    let mut app = app();
    focus(&mut app, A, "");
    press(&mut app, KeyCode::Digit1, Key::Character("1".into()));
    press(&mut app, KeyCode::Enter, Key::Enter);
    press(&mut app, KeyCode::Digit2, Key::Character("2".into()));
    app.update();
    assert_eq!(messages(&mut app), vec![FieldMsg { field: A, event: FieldEvent::Changed(TextDraft::new("1", false)) }, FieldMsg { field: A, event: FieldEvent::Submit("1".into()) }], "keys after Enter are dropped");
    assert!(is_typing(&mut app));
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Enter), "Enter is consumed");
    press(&mut app, KeyCode::Escape, Key::Escape);
    app.update();
    assert_eq!(messages(&mut app), vec![FieldMsg { field: A, event: FieldEvent::Cancel }]);
    assert!(!is_typing(&mut app));
    // Shift+Enter is a newline in a ShiftNewline field.
    focus(&mut app, B, "a");
    hold(&mut app, KeyCode::ShiftLeft, ButtonState::Pressed);
    press(&mut app, KeyCode::Enter, Key::Enter);
    hold(&mut app, KeyCode::ShiftLeft, ButtonState::Released);
    press(&mut app, KeyCode::Enter, Key::Enter);
    app.update();
    let got = messages(&mut app);
    assert_eq!(got.last(), Some(&FieldMsg { field: B, event: FieldEvent::Submit("a\n".into()) }), "{got:?}");
}

/// A press elsewhere takes the keyboard from a field (`Blur`).
#[test]
fn a_press_elsewhere_blurs() {
    let mut app = app();
    focus(&mut app, A, "");
    app.world_mut().write_message(MouseButtonInput { button: MouseButton::Left, state: ButtonState::Pressed, window: Entity::PLACEHOLDER });
    app.update();
    assert!(!is_typing(&mut app));
    assert_eq!(messages(&mut app), vec![FieldMsg { field: A, event: FieldEvent::Blur }]);
}

/// Files that still read keyboard messages themselves, each moving to the
/// kit field in this epic (one-text-entry); the list only shrinks.
const NOT_YET_MOVED: &[&str] = &[
    "app/picker/mod.rs",
    "builder/drafts.rs",
    "cad/attach.rs",
    "cad/display/entry.rs",
    "cad/files/form.rs",
    "cad/inspector/editors.rs",
    "cad/inspector/entry.rs",
    "cad/materials/panel.rs",
    "cad/numeric.rs",
    "cad/panel.rs",
    "cad/results/forms.rs",
    "cad/surfaces/form.rs",
    "cad/surfaces/palette.rs",
    "cad/views/panel.rs",
    "lesson/actions.rs",
    "robot/panel_ui.rs",
];

/// Nothing outside `ui_kit/text/` reads keyboard messages for text
/// (`MessageReader`, `MessageCursor` or `Messages` of `KeyboardInput`, or a
/// `FocusedInput` observer of it): text goes through the kit field.
#[test]
fn keyboard_text_is_read_only_in_the_kit() {
    // Built so this file never contains the needle literally.
    let needle = ["Keyboard", "Input>"].concat();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let kit = src.join("ui_kit").join("text");
    let mut offenders = Vec::new();
    let mut dirs = vec![src.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path != kit {
                    dirs.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                let relative = path.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
                let text = std::fs::read_to_string(&path).unwrap();
                let hits: Vec<String> = text.lines().enumerate().filter(|(_, l)| !l.trim_start().starts_with("//") && l.contains(needle.as_str())).map(|(i, l)| format!("{relative}:{}: {}", i + 1, l.trim())).collect();
                if !hits.is_empty() && !NOT_YET_MOVED.contains(&relative.as_str()) {
                    offenders.extend(hits);
                }
            }
        }
    }
    offenders.sort();
    assert!(offenders.is_empty(), "read text through ui_kit::text (TextField, FieldMsg), not keyboard messages:\n{}", offenders.join("\n"));
}
