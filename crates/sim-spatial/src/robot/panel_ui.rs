//! The robot inspector's UI-only state and its in-window controls that need
//! more than a button (window-first-usability):
//!
//! - **More recordings.** The Replay block lists the five most recent
//!   recordings; "More recordings…" expands it to every recording (each a
//!   Replay button with the same `RobotAction::Replay` as the visible ones).
//!   The expanded flag is view state, not an intent, so it is a
//!   [`PanelToggle`] read here rather than a `RobotAction`.
//! - **Recorded seek.** A kit slider over the recorded timeline
//!   ([`RecordedSeek`]): a press or drag writes `RobotAction::Recorded
//!   {Seek {t}}` (the REST `robot_recorded` seek), only while held
//!   (`ui_kit::slider_held`) and only when the time changes.
//! - **Compiled gait path.** The kit path field ("Open a compiled gait")
//!   writes `RobotAction::Gait {Open {source: Path}}`, the action REST
//!   `robot_gait {path}` parses into. It is the kit field [`GAIT_PATH`]
//!   (`ui_kit::text`): Enter submits, Escape or a press elsewhere ends it.
//!   While it has the keyboard, robot mode's keys (`actions::keys`) and the
//!   shared camera's keys are not read (`ui_kit::text::typing`). The field
//!   refuses the keyboard while the Leg calibration panel is shown (its Q/A
//!   hold-to-move and Z STOP keys are read whatever has focus) or the
//!   document picker is open, and gives it up when either opens or the
//!   field itself is gone. The directory listing is read on
//!   `Pool::Io` (`path_field::request`); the UI thread never reads a
//!   directory. "Open" is enabled without touching the filesystem
//!   (`open_enabled`); the file's existence is checked once per submit.
use super::*;
use crate::app::actions::Act;
use crate::jobs::Latest;
use crate::ui_kit::path_field::{self, Listing, PathHit, PathView};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFieldApp, TextFocus, Typing};

/// The suffixes the gait path field lists.
const GAIT_SUFFIXES: [&str; 1] = ["json"];
const GAIT_FIELD_LABEL: &str = "Open a compiled gait (compiled.json)";
const HARDWARE_HAS_KEYS: &str = "The Leg calibration panel is open and its keys (Q/A move, Z/Escape STOP) stay live: close it to type a path.";

/// The gait path field (one-line, not sticky: a press elsewhere ends it).
pub(crate) const GAIT_PATH: FieldId = FieldId("robot.gait_path");

/// Spawn the gait path field (`RobotPlugin`).
pub(super) fn add_field(app: &mut App) {
    app.add_message::<FieldMsg>().add_text_field(GAIT_PATH, TextField::new(GAIT_FIELD_LABEL));
}

/// The gait path field's text (the kit field's draft, followed here for
/// the listing and Open) and notice. Whether it has the keyboard is the
/// kit's (`Typing::focused(GAIT_PATH)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct GaitPathDraft {
    pub draft: TextDraft,
    /// Why the last submit or focus press did nothing.
    pub notice: Option<String>,
    /// The listing key last asked for (`path_field::listing_key`).
    asked: Option<String>,
}

/// Robot mode's UI-only state (kept across robot documents in the window;
/// the kit takes the field's keyboard when robot mode is left).
#[derive(Resource, Default)]
pub(super) struct RobotPanelUi {
    /// The Replay block lists every recording, not only the five most recent.
    pub recordings_expanded: bool,
    pub gait_path: GaitPathDraft,
    listing: Latest<Listing>,
    listed: Option<Listing>,
    /// Bumped whenever `listed` changes (the field's redraw key, instead of formatting the listing).
    listed_rev: u64,
}
impl RobotPanelUi {
    /// The listing of the directory the typed path names, when it is the one asked for now.
    fn listing(&self) -> Option<&Listing> {
        let (key, _) = path_field::listing_key(&self.gait_path.draft.text, &GAIT_SUFFIXES)?;
        self.listed.as_ref().filter(|l| l.key == key)
    }
}

/// A UI-only toggle of the inspector.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PanelToggle {
    /// "More recordings…" / "Fewer recordings".
    Recordings,
}

/// A part of the gait path field.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GaitPathPart(pub PathHit);

/// Where the gait path field is drawn (spawned by `controls::gait_panel`).
#[derive(Component)]
pub(super) struct GaitPathRoot;

/// The recorded timeline's seek slider (a kit slider).
#[derive(Component)]
pub(super) struct RecordedSeek;

/// The seek slider's fill (the played fraction).
#[derive(Component)]
pub(super) struct RecordedSeekFill;

/// Whether "Open" can be pressed for `text`, without touching the filesystem
/// (this runs every Present frame): a path that names a file (non-empty, not
/// ending in '/'), and nothing refusing a gait open for reasons other than the
/// file (planar file, no preset scene, recorded preset, a physics run running,
/// a replay in progress, a gait still loading). Whether the file exists is
/// checked once on submit (`check`), and its refusal shows under the field.
fn open_enabled(view: &RobotView, text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || text.ends_with('/') {
        return false;
    }
    // Speed passes the preview's own check: only the common refusals remain (none stats a file).
    if check(view, &RobotAction::Gait { action: GaitAction::Speed { scale: 1.0 } }).is_err() {
        return false;
    }
    let Some(r) = view.run.as_ref() else { return false };
    let running = r.check(RunAction::Pause).is_ok();
    let replaying = r.replay_state().phase == ReplayPhase::Replaying;
    let loading = r.gait_preview().is_some_and(|g| g.phase() == gait::GaitPhase::Loading);
    !running && !replaying && !loading
}

/// The open-path action for `text` (`~/` expanded), or None for an empty path.
fn open_action(text: &str) -> Option<RobotAction> {
    let path = path_field::expand(text.trim());
    (!path.is_empty()).then(|| RobotAction::Gait { action: GaitAction::Open { source: GaitSource::Path(path) } })
}

/// The recorded time a slider fraction stands for, within the capture's
/// [first, last] frame times (clamped, so the end never overshoots by an ulp).
pub(super) fn seek_time(times: &[f64], fraction: f32) -> Option<f64> {
    let (first, last) = (*times.first()?, *times.last()?);
    let f = f64::from(fraction.clamp(0.0, 1.0));
    Some((first + f * (last - first)).clamp(first, last))
}

/// The fraction of the recorded timeline at time `t`.
pub(super) fn seek_fraction(times: &[f64], t: f64) -> f32 {
    match (times.first(), times.last()) {
        (Some(first), Some(last)) if last > first => ((t - first) / (last - first)).clamp(0.0, 1.0) as f32,
        _ => 0.0,
    }
}

/// Input: the inspector's toggles (UI state only).
pub(super) fn toggles(presses: Query<&PanelToggle, With<crate::ui_kit::activation::Activated>>, mut ui: ResMut<RobotPanelUi>) {
    for toggle in &presses {
        match toggle {
            PanelToggle::Recordings => ui.recordings_expanded = !ui.recordings_expanded,
        }
    }
}

/// Input: a press or drag on the recorded seek slider writes
/// `Recorded {Seek {t}}` when the time changes; nothing while it is not held.
pub(super) fn recorded_seek(bars: Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction), With<RecordedSeek>>, view: Res<RobotView>, mut sent: Local<Option<f64>>, mut out: MessageWriter<Act<RobotAction>>) {
    let mut held = false;
    for (value, pressed, interaction) in &bars {
        if !crate::ui_kit::slider_held(pressed, interaction) {
            continue;
        }
        held = true;
        let Some(p) = view.run.as_ref().and_then(|r| r.playback()) else { continue };
        let Some(t) = seek_time(&p.timeline().times, value.0) else { continue };
        if *sent != Some(t) {
            *sent = Some(t);
            // Gated like the transport buttons: a refused seek is not resent every drag frame.
            let action = RobotAction::Recorded { action: RecordedAction::Seek { t } };
            if check(&view, &action).is_ok() {
                out.write(Act::ui(action));
            }
        }
    }
    if !held {
        *sent = None;
    }
}

/// Input: the gait path field's presses and its kit field's messages (see
/// the module doc). Typing (`Changed`) is followed into the draft (the
/// listing follows it); Enter (`Submit`) or the Open button submits, and an
/// accepted path takes the keyboard away; Escape (`Cancel`) and a press
/// elsewhere (`Blur`) have already taken it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn gait_path_input(
    mut ui: ResMut<RobotPanelUi>,
    view: Res<RobotView>,
    parts: Query<&GaitPathPart, With<crate::ui_kit::activation::Activated>>,
    // The field's messages are read first, then `TextFocus` acts (a `ParamSet`: one at a time).
    mut field: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    hardware: Option<Res<crate::robot::hardware::Hardware>>,
    (roots, picker): (Query<(), With<GaitPathRoot>>, Option<Res<crate::app::picker::Picker>>),
    mut out: MessageWriter<Act<RobotAction>>,
) {
    let events: Vec<FieldEvent> = field.p0().read().filter(|m| m.field == GAIT_PATH).map(|m| m.event.clone()).collect();
    let mut text = field.p1();
    let before = ui.gait_path.clone();
    let mut f = before.clone();
    let mut focused = text.focused(GAIT_PATH);
    let panel_open = hardware.is_some_and(|h| h.open);
    // The document picker has the window (and its own path field) while open.
    let picker_open = picker.is_some_and(|p| p.open.is_some());
    let mut submit = false;
    // This frame's typing (the kit read it in PreUpdate, before these presses).
    for event in events {
        match event {
            FieldEvent::Changed(draft) => {
                f.draft = draft;
                f.notice = None;
            }
            FieldEvent::Submit(_) => submit = true,
            FieldEvent::Cancel | FieldEvent::Tab { .. } | FieldEvent::Arrow { .. } | FieldEvent::Blur => {}
        }
    }
    for part in &parts {
        match part.0 {
            PathHit::Field if !focused => {
                if picker_open {
                    continue;
                }
                if panel_open {
                    f.notice = Some(HARDWARE_HAS_KEYS.into());
                    continue;
                }
                f.draft.select_all = true;
                f.notice = None;
                // Read the directory again: files may have appeared since.
                f.asked = None;
                // An empty field starts in the workspace root (a compiled.json may be relative to it).
                if f.draft.text.is_empty()
                    && let Ok(root) = &view.root
                {
                    f.draft.text = format!("{}/", root.display().to_string().trim_end_matches('/'));
                    f.draft.select_all = false;
                }
                focused = text.focus_draft(GAIT_PATH, f.draft.clone());
            }
            PathHit::Field => {}
            PathHit::Entry(i) => {
                // Only the listing drawn for the path as it was (a newer path's older listing never fills it).
                let picked = ui.listing().and_then(|l| l.entries.get(i).map(|(n, d)| (l.dir.clone(), n.clone(), *d)));
                if let Some((dir, name, is_dir)) = picked {
                    f.draft.text = path_field::pick(&f.draft.text, &dir, &name, is_dir, false);
                    f.draft.select_all = false;
                    f.notice = None;
                }
            }
            PathHit::Up => {
                f.draft.text = path_field::up(&f.draft.text, false);
                f.draft.select_all = false;
                f.notice = None;
            }
            PathHit::Submit => submit = true,
        }
    }
    // The Leg calibration panel or the document picker opening, or the field
    // going away (another view's inspector), ends the typing.
    if focused && (panel_open || picker_open || roots.is_empty()) {
        text.blur(GAIT_PATH);
    }
    if submit {
        match open_action(&f.draft.text) {
            None => f.notice = Some("Type the path of a compiled.json (absolute, ~/, or relative to the workspace root).".into()),
            Some(action) => match check(&view, &action) {
                Ok(()) => {
                    out.write(Act::ui(action));
                    text.blur(GAIT_PATH);
                    f.notice = None;
                }
                Err(why) => f.notice = Some(why),
            },
        }
    }
    // A listing pick or Up edits the field's draft too (no change: nothing written).
    text.set(GAIT_PATH, f.draft.clone());
    // The listing follows the typed directory (read on Pool::Io).
    let ask = path_field::listing_key(&f.draft.text, &GAIT_SUFFIXES).filter(|(key, _)| f.asked.as_deref() != Some(key.as_str()));
    if let Some((key, dir)) = ask {
        f.asked = Some(key.clone());
        path_field::request(&mut ui.listing, "robot gait path listing", key, dir, GAIT_SUFFIXES.iter().map(|s| s.to_string()).collect());
    }
    if f != before {
        ui.gait_path = f;
    }
}

/// JobResults: a finished gait path listing.
pub(super) fn receive_listing(mut ui: ResMut<RobotPanelUi>) {
    if ui.listing.pending().is_none() {
        return;
    }
    let ui = &mut *ui;
    if path_field::receive(&mut ui.listing, &mut ui.listed) {
        ui.listed_rev += 1;
    }
}

/// Present: the gait path field, rebuilt when its draft, focus, listing or
/// submit state changes (or its root is new).
pub(super) fn gait_path_draw(mut commands: Commands, ui: Res<RobotPanelUi>, typing: Typing, view: Res<RobotView>, fonts: Res<UiFonts>, roots: Query<Entity, With<GaitPathRoot>>, mut last: Local<Option<(Entity, GaitPathDraft, bool, u64, bool)>>) {
    let Ok(root) = roots.single() else {
        *last = None;
        return;
    };
    let f = &ui.gait_path;
    let focused = typing.focused(GAIT_PATH);
    // No filesystem access and no formatting per frame: the draft (whose text
    // selects the listing), the focus, the listing's revision and the Open state are the key.
    let enabled = open_enabled(&view, &f.draft.text);
    if last.as_ref().is_some_and(|(e, d, foc, rev, en)| *e == root && d == f && *foc == focused && *rev == ui.listed_rev && *en == enabled) {
        return;
    }
    *last = Some((root, f.clone(), focused, ui.listed_rev, enabled));
    let listing = ui.listing();
    let k = Kit { f: &fonts };
    let field = PathView {
        label: GAIT_FIELD_LABEL,
        text: &f.draft.text,
        placeholder: "/path/to/compiled.json, or relative to the workspace root",
        focused,
        selected: f.draft.select_all,
        submit: Some("Open"),
        submit_enabled: enabled,
        listing,
    };
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|p| {
        k.path_field(p, &field, GaitPathPart);
        if let Some(notice) = &f.notice {
            p.spawn(k.text(notice.clone(), size::CAPTION, WARN, 0));
        }
    });
}
