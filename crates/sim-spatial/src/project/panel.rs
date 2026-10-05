//! The project in the window: the bottom strip's robot button and step
//! chips (every mode), and the project card above the strip.
//!
//! - **Strip**: `◆ <robot>` (opens or closes the card) and one chip per step
//!   (`✓ 1 Design`, `● 2 Model`, `! 3 Test`, `○ 4 Learn`…: done, next,
//!   needs attention, waiting); a chip goes to its step. Without a project:
//!   `◆ New robot…`.
//! - **Card**: without a project, Start (a robot's name and what it should
//!   do → Create; Open a project by path; recent projects). With one: the
//!   description, the steps (each with its line and what to do), and the
//!   chosen step's detail: Design (the conversation with the design
//!   assistant), Model (Make model; every assumption, blocking first),
//!   Test (the criteria and the latest outcomes; Run test), Learn (this
//!   robot's lessons and suggestions; Write), Make (Export parts).
//!
//! Every button writes a [`ProjectAction`], the same as REST `project_*`.
use super::status::State;
use super::{ProjectAction, ProjectState, Step};
use crate::app::Persistent;
use crate::app::actions::Act;
use crate::ui_kit::wheel_delta;
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFieldApp, TextFocus, Typing};
use crate::ui_kit::{ACCENT, BORDER, DANGER, FAINT, Kit, Look, OK, SUBTLE, SURFACE, TEXT, UiFonts, WARN, size};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use serde_json::{Value, json};

pub(crate) const NAME: FieldId = FieldId("project.name");
pub(crate) const DESCRIPTION: FieldId = FieldId("project.description");
pub(crate) const CHAT: FieldId = FieldId("project.chat");
pub(crate) const PATH: FieldId = FieldId("project.path");
const CARD_WIDTH: f32 = 460.0;

/// The card's state (open, view, the fields' drafts).
#[derive(Default, Clone, Debug)]
pub(crate) struct PanelState {
    pub open: bool,
    pub view: Option<Step>,
}
impl PanelState {
    pub(crate) fn json(&self) -> Value {
        json!({"open": self.open, "view": self.view})
    }
}

/// What the fields hold (kept here so they survive rebuilds).
#[derive(Resource, Default)]
pub(crate) struct Drafts {
    name: String,
    description: String,
    chat: String,
    path: String,
}

/// The strip's project area (spawned by `app::switcher`).
#[derive(Component)]
pub(crate) struct ProjectStripRoot;
#[derive(Component)]
struct CardRoot;
#[derive(Component)]
struct CardScroll;

/// A press in the strip or the card.
#[derive(Component, Clone, Debug, PartialEq)]
enum Press {
    Panel,
    Close,
    Step(Step),
    View(Step),
    Create,
    Open,
    Recent(String),
    Send,
    Export,
    Test,
    Write(String),
    Make,
    Cancel,
    Field(FieldId),
}

/// Projects under `<workspace>/projects` (newest first), for Start.
pub(crate) fn recent_projects() -> Vec<Value> {
    let Ok(dir) = crate::workspace::path("projects") else { return Vec::new() };
    let mut found: Vec<(std::time::SystemTime, Value)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = sim_runtime::robot_project::Project::open(&e.path()).ok()?;
            let t = std::fs::metadata(&p.path).and_then(|m| m.modified()).ok()?;
            Some((t, json!({"name": p.file.name, "path": p.path, "description": p.file.description})))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().take(8).map(|(_, v)| v).collect()
}

fn colour(state: State) -> Color {
    match state {
        State::Done => OK,
        State::Ready => ACCENT,
        State::Attention => WARN,
        State::Waiting => FAINT,
    }
}

/// Present: the strip's project area, rebuilt when the project changes.
fn strip(mut commands: Commands, st: Res<ProjectState>, fonts: Res<UiFonts>, roots: Query<Entity, With<ProjectStripRoot>>, mut last: Local<Option<(Entity, u64)>>) {
    let Ok(root) = roots.single() else { return };
    if *last == Some((root, st.revision)) {
        return;
    }
    *last = Some((root, st.revision));
    commands.entity(root).despawn_related::<Children>();
    let k = Kit::new(&fonts);
    commands.entity(root).with_children(|p| {
        match &st.open {
            None => {
                p.spawn(k.button("◆ New robot…", Press::Panel, Look::Secondary, true));
            }
            Some(project) => {
                p.spawn(k.button(&format!("◆ {}", project.file.name), Press::Panel, Look::Chip(st.panel.open), true));
                for s in &st.steps {
                    p.spawn(k.chip(&format!("{} {} {}", s.state.mark(), s.step.number(), s.step.label()), Press::Step(s.step), st.panel.view == Some(s.step), true));
                }
            }
        }
    });
}

/// Present: the card above the strip, rebuilt when the project or a field's focus changes.
#[allow(clippy::too_many_arguments)]
fn card(mut commands: Commands, st: Res<ProjectState>, drafts: Res<Drafts>, typing: Typing, fonts: Res<UiFonts>, roots: Query<Entity, With<CardRoot>>, mut last: Local<Option<String>>) {
    let focus = [NAME, DESCRIPTION, CHAT, PATH].map(|f| typing.focused(f));
    let key = format!("{}|{}|{focus:?}|{}|{}", st.revision, st.panel.open, drafts.is_changed(), roots.iter().count());
    if last.as_deref() == Some(key.as_str()) && !drafts.is_changed() {
        return;
    }
    *last = Some(key);
    for r in &roots {
        commands.entity(r).despawn();
    }
    if !st.panel.open {
        return;
    }
    let k = Kit::new(&fonts);
    let node = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(12.0),
        bottom: Val::Px(crate::ui_kit::SWITCHER_STRIP + 8.0),
        width: Val::Px(CARD_WIDTH),
        max_height: Val::Percent(78.0),
        flex_direction: FlexDirection::Column,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(8.0)),
        padding: UiRect::all(Val::Px(12.0)),
        row_gap: Val::Px(8.0),
        ..default()
    };
    commands.spawn((node, BackgroundColor(SURFACE), BorderColor::all(BORDER), GlobalZIndex(38), Persistent, CardRoot)).with_children(|c| {
        // Header.
        c.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|h| {
            let title = st.open.as_ref().map_or("Start a robot".to_string(), |p| p.file.name.clone());
            h.spawn((k.text(title, size::TITLE, TEXT, 2), Node { flex_grow: 1.0, ..default() }));
            h.spawn(k.button("Close", Press::Close, Look::Ghost, true));
        });
        c.spawn((k.scroll_area(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), min_height: Val::Px(0.0), flex_shrink: 1.0, ..default() }, 0.0), CardScroll)).with_children(|body| match &st.open {
            None => start(body, &k, &drafts, focus),
            Some(_) => project(body, &k, &st, &drafts, focus[2]),
        });
        if let Some(m) = &st.message {
            let (text, col) = match m {
                Ok(t) => (t.as_str(), SUBTLE),
                Err(e) => (e.as_str(), DANGER),
            };
            c.spawn(k.text(text, size::CAPTION, col, 0));
        }
    });
}

fn start(p: &mut ChildSpawnerCommands, k: &Kit, d: &Drafts, focus: [bool; 4]) {
    p.spawn(k.text("Describe a robot and the design assistant builds it in CAD; then you make its physics model, test it, learn about it and get its parts.", size::SMALL, SUBTLE, 0));
    p.spawn(k.section("New robot"));
    p.spawn(k.text("Name", size::CAPTION, SUBTLE, 0));
    p.spawn(k.input(&d.name, "Lift arm", Press::Field(NAME), focus[0]));
    p.spawn(k.text("What should it do?", size::CAPTION, SUBTLE, 0));
    p.spawn(k.input(&d.description, "A one-joint arm that lifts a 20 g weight to 60° in one second", Press::Field(DESCRIPTION), focus[1]));
    p.spawn(k.button("Create robot", Press::Create, Look::Primary, !d.name.trim().is_empty()));
    p.spawn(k.section("Open a robot"));
    p.spawn(k.input(&d.path, "/path/to/robot-folder or name.robot.json", Press::Field(PATH), focus[3]));
    p.spawn(k.button("Open", Press::Open, Look::Secondary, !d.path.trim().is_empty()));
    let recent = recent_projects();
    if !recent.is_empty() {
        p.spawn(k.text("Recent", size::CAPTION, SUBTLE, 0));
        for r in recent {
            let label = format!("{} — {}", r["name"].as_str().unwrap_or(""), r["description"].as_str().unwrap_or(""));
            p.spawn(k.button(&label, Press::Recent(r["path"].as_str().unwrap_or("").to_string()), Look::Ghost, true));
        }
    }
}

fn project(p: &mut ChildSpawnerCommands, k: &Kit, st: &ProjectState, d: &Drafts, chat_focus: bool) {
    let Some(project) = &st.open else { return };
    if !project.file.description.is_empty() {
        p.spawn(k.text(&project.file.description, size::SMALL, SUBTLE, 0));
    }
    // The steps.
    for s in &st.steps {
        p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), align_items: AlignItems::FlexStart, ..default() }).with_children(|r| {
            r.spawn(k.button(&format!("{} {} {}", s.state.mark(), s.step.number(), s.step.label()), Press::View(s.step), Look::Tab(st.panel.view == Some(s.step)), true));
            r.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_shrink: 1.0, min_width: Val::Px(0.0), ..default() }).with_children(|t| {
                t.spawn(k.text(&s.line, size::SMALL, colour(s.state), 0));
                if let Some(h) = &s.hint {
                    t.spawn(k.text(h, size::CAPTION, FAINT, 0));
                }
            });
        });
    }
    if !st.intents.is_empty() {
        p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(k.text(format!("Waiting: {}", st.intents.iter().map(intent_label).collect::<Vec<_>>().join(", ")), size::CAPTION, WARN, 0));
            r.spawn(k.button("Cancel", Press::Cancel, Look::Ghost, true));
        });
    }
    let view = st.panel.view.or_else(|| super::status::next(&st.steps)).unwrap_or(Step::Design);
    p.spawn(k.section(&format!("{} {}", view.number(), view.label())));
    p.spawn(k.text(view.about(), size::CAPTION, FAINT, 0));
    let f = st.files.as_ref();
    match view {
        Step::Design => {
            let turns = &project.file.chat;
            if turns.is_empty() {
                p.spawn(k.text("Tell the design assistant what to build or change. It edits the design in CAD (each change can be undone) and explains what it did.", size::SMALL, SUBTLE, 0));
            }
            for t in turns.iter().rev().take(6).rev() {
                let (who, col) = if t.role == "person" { ("You", TEXT) } else { ("Assistant", ACCENT) };
                p.spawn(k.text(format!("{who}: {}", clip(&t.text, 700)), size::SMALL, col, 0));
            }
            if let Some(line) = st.chat.line() {
                p.spawn(k.text(line, size::CAPTION, WARN, 0));
            }
            p.spawn(k.input(&d.chat, "Ask for a change: “make the arm 20 mm longer”", Press::Field(CHAT), chat_focus));
            p.spawn(k.button("Send", Press::Send, Look::Primary, !d.chat.trim().is_empty() && !st.chat.busy()));
        }
        Step::Model => {
            let unsaved = st.steps.first().is_some_and(|s| s.line.contains("unsaved"));
            p.spawn(k.button(if unsaved { "Save design and make model" } else { "Make model" }, Press::Export, Look::Primary, true));
            if let Some(f) = f {
                let blocking: Vec<&Value> = f.assumptions.iter().filter(|a| a["blocking"] == json!(true)).collect();
                let others: Vec<&Value> = f.assumptions.iter().filter(|a| a["blocking"] != json!(true)).collect();
                for a in &blocking {
                    p.spawn(k.text(format!("Must set: {}", a["what"].as_str().unwrap_or("")), size::SMALL, WARN, 1));
                    p.spawn(k.text(format!("How: {}", a["fix"].as_str().unwrap_or("")), size::CAPTION, SUBTLE, 0));
                }
                if !others.is_empty() {
                    p.spawn(k.text(format!("Estimates in the model ({}):", others.len()), size::CAPTION, SUBTLE, 1));
                    for a in others.iter().take(6) {
                        p.spawn(k.text(format!("• {}", clip(a["what"].as_str().unwrap_or(""), 220)), size::CAPTION, FAINT, 0));
                    }
                    if others.len() > 6 {
                        p.spawn(k.text(format!("…and {} more, listed in the model file", others.len() - 6), size::CAPTION, FAINT, 0));
                    }
                }
            }
        }
        Step::Test => {
            let running = st.test.as_ref().map(|t| *t.progress.lock().unwrap_or_else(|e| e.into_inner()));
            p.spawn(k.button(&running.map_or("Run test".to_string(), |x| format!("Running… {:.0} %", x * 100.0)), Press::Test, Look::Primary, running.is_none()));
            match &project.file.test {
                None => {
                    p.spawn(k.text("No test stated yet: the first run makes a starting test from the robot's joints (move toward the upper limit in a second, hold, keep torque and temperature in reserve, and printed parts twice as strong as the peak loads need), saved to edit.", size::SMALL, SUBTLE, 0));
                }
                Some(t) => {
                    p.spawn(k.text(format!("“{}”, {} s:", t.name, t.duration_s), size::SMALL, TEXT, 1));
                    // Outcomes are shown against the criteria only while the report still describes what would run.
                    let current = f.is_some_and(|f| matches!(f.standing, Some(sim_runtime::system_evidence::Standing::Current { .. })));
                    let outcomes = f.and_then(|f| f.report.as_ref()).filter(|_| current).map(|r| r["outcomes"].as_array().cloned().unwrap_or_default()).unwrap_or_default();
                    for (i, c) in t.criteria.iter().enumerate() {
                        let o = outcomes.get(i).filter(|o| o["criterion"] == serde_json::to_value(c).unwrap_or_default());
                        let (mark, col) = match o.and_then(|o| o["status"].as_str()) {
                            Some("pass") => ("✓", OK),
                            Some("fail") => ("✗", DANGER),
                            Some(_) => ("–", WARN),
                            None => ("·", FAINT),
                        };
                        p.spawn(k.text(format!("{mark} {}", c.describe()), size::SMALL, col, 0));
                        if let Some(detail) = o.and_then(|o| o["detail"].as_str()) {
                            p.spawn(k.text(format!("   {detail}"), size::CAPTION, FAINT, 0));
                        }
                    }
                }
            }
            if let Some(r) = f.and_then(|f| f.report.as_ref()) {
                let stale = f.and_then(|f| match &f.standing {
                    Some(sim_runtime::system_evidence::Standing::Stale { changed, .. }) => Some(changed.join("; ")),
                    _ => None,
                });
                let col = if stale.is_some() { WARN } else if r["verdict"] == "passed" { OK } else if r["verdict"] == "failed" { DANGER } else { WARN };
                match &stale {
                    Some(changed) => p.spawn(k.text(format!("The last run no longer counts: {changed}. It said: {}", r["summary"].as_str().unwrap_or("")), size::SMALL, col, 1)),
                    None => p.spawn(k.text(r["summary"].as_str().unwrap_or(""), size::SMALL, col, 1)),
                };
                if let Some(c) = r["system"]["controllers"].as_array() {
                    let ran = if c.is_empty() { "no controller blocks: the test bench commanded the robot's servo targets directly".to_string() } else { format!("with the system's controllers {}", c.iter().filter_map(|b| b["block"].as_str()).collect::<Vec<_>>().join(", ")) };
                    p.spawn(k.text(format!("Ran the project's system, {ran}."), size::CAPTION, FAINT, 0));
                }
                if let Some(n) = r["not_modelled"].as_array().filter(|n| !n.is_empty()) {
                    p.spawn(k.text(format!("Not modelled: {}", n.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")), size::CAPTION, FAINT, 0));
                }
            }
        }
        Step::Learn => {
            if let Some(f) = f
                && !f.lessons.is_empty()
            {
                p.spawn(k.text("This robot's lessons:", size::CAPTION, SUBTLE, 1));
                for (_, title) in &f.lessons {
                    p.spawn(k.text(format!("• {title}"), size::SMALL, TEXT, 0));
                }
                p.spawn(k.button("Open in Lessons", Press::Step(Step::Learn), Look::Secondary, true));
            }
            if let Some(w) = &st.lessons.writing {
                p.spawn(k.text(format!("Writing “{}”…", w.title), size::SMALL, WARN, 0));
            }
            p.spawn(k.text("Suggested for this robot:", size::CAPTION, SUBTLE, 1));
            for s in super::lessons::suggestions(st) {
                p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), align_items: AlignItems::FlexStart, ..default() }).with_children(|r| {
                    r.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_shrink: 1.0, min_width: Val::Px(0.0), ..default() }).with_children(|t| {
                        t.spawn(k.text(s["title"].as_str().unwrap_or(""), size::SMALL, TEXT, 1));
                        t.spawn(k.text(s["why"].as_str().unwrap_or(""), size::CAPTION, FAINT, 0));
                        if let Some(g) = s["general_lesson"].as_str() {
                            t.spawn(k.text(format!("General lesson in the library: {g}"), size::CAPTION, FAINT, 0));
                        }
                    });
                    let written = s["written"] == json!(true);
                    r.spawn(k.button(if written { "Written" } else { "Write" }, Press::Write(s["id"].as_str().unwrap_or("").to_string()), Look::Secondary, !written && st.lessons.writing.is_none()));
                });
            }
        }
        Step::Make => {
            p.spawn(k.button("Export parts", Press::Make, Look::Primary, true));
            if let Some(m) = f.and_then(|f| f.make.as_ref()) {
                for part in m["parts"].as_array().into_iter().flatten() {
                    p.spawn(k.text(format!("• {} ({}, {:.1} g) → {}", part["name"].as_str().unwrap_or(""), part["material"].as_str().unwrap_or(""), part["mass_kg"].as_f64().unwrap_or(0.0) * 1000.0, part["file"].as_str().unwrap_or("")), size::SMALL, TEXT, 0));
                }
                for b in m["bought"].as_array().into_iter().flatten() {
                    p.spawn(k.text(format!("• buy: {} ({})", b["name"].as_str().unwrap_or(""), b["spec"].as_str().unwrap_or("")), size::SMALL, SUBTLE, 0));
                }
            }
            p.spawn(k.text(super::make::NOT_PORTED, size::CAPTION, WARN, 0));
        }
    }
}

fn intent_label(i: &super::Intent) -> String {
    match i {
        super::Intent::Export { .. } => "make the model (needs CAD on this robot)".into(),
        super::Intent::Ask(_) => "ask the design assistant (needs CAD on this robot)".into(),
        super::Intent::Test => "run the test (needs a current model)".into(),
        super::Intent::Make => "export parts (needs CAD on this robot)".into(),
        super::Intent::OpenLessons(s) => format!("open lesson {s}"),
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max { text.to_string() } else { format!("{}…", text.chars().take(max).collect::<String>()) }
}

/// Input: presses become project actions; fields keep their drafts.
fn input(presses: Query<&Press, With<crate::ui_kit::activation::Activated>>, mut msgs: MessageReader<FieldMsg>, mut text: TextFocus, mut drafts: ResMut<Drafts>, mut out: MessageWriter<Act<ProjectAction>>) {
    for m in msgs.read() {
        let slot = match m.field {
            NAME => &mut drafts.name,
            DESCRIPTION => &mut drafts.description,
            CHAT => &mut drafts.chat,
            PATH => &mut drafts.path,
            _ => continue,
        };
        match &m.event {
            FieldEvent::Changed(d) if *slot != d.text => *slot = d.text.clone(),
            FieldEvent::Submit(_) => {
                text.blur(m.field);
                if m.field == CHAT && !drafts.chat.trim().is_empty() {
                    out.write(Act::ui(ProjectAction::ProjectChat { text: std::mem::take(&mut drafts.chat) }));
                }
            }
            FieldEvent::Cancel => text.blur(m.field),
            _ => {}
        }
    }
    for press in &presses {
        let action = match press {
            Press::Panel => ProjectAction::ProjectPanel { open: None, view: None },
            Press::Close => ProjectAction::ProjectPanel { open: Some(false), view: None },
            Press::Step(s) => ProjectAction::ProjectStep { step: *s },
            Press::View(s) => ProjectAction::ProjectPanel { open: Some(true), view: Some(*s) },
            Press::Create => ProjectAction::ProjectNew { name: drafts.name.trim().to_string(), description: Some(drafts.description.trim().to_string()).filter(|d| !d.is_empty()), dir: None },
            Press::Open => ProjectAction::ProjectOpen { path: drafts.path.trim().into(), step: None },
            Press::Recent(path) => ProjectAction::ProjectOpen { path: path.into(), step: None },
            Press::Send => ProjectAction::ProjectChat { text: std::mem::take(&mut drafts.chat) },
            Press::Export => ProjectAction::ProjectExport,
            Press::Test => ProjectAction::ProjectTest,
            Press::Write(id) => ProjectAction::ProjectLessons { op: "write".into(), topic: Some(id.clone()) },
            Press::Make => ProjectAction::ProjectMake,
            Press::Cancel => ProjectAction::ProjectCancel,
            Press::Field(f) => {
                if !text.focused(*f) {
                    let shown = match *f {
                        NAME => drafts.name.clone(),
                        DESCRIPTION => drafts.description.clone(),
                        CHAT => drafts.chat.clone(),
                        _ => drafts.path.clone(),
                    };
                    text.focus_draft(*f, TextDraft::new(shown, false));
                }
                continue;
            }
        };
        if matches!(action, ProjectAction::ProjectNew { .. }) {
            drafts.name.clear();
            drafts.description.clear();
        }
        out.write(Act::ui(action));
    }
}

/// The wheel over the card scrolls it.
fn scroll(mut wheel: MessageReader<MouseWheel>, windows: Query<&Window, With<bevy::window::PrimaryWindow>>, mut areas: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform, &mut ScrollPosition), With<CardScroll>>) {
    let delta = wheel_delta(&mut wheel, crate::ui_kit::WHEEL_LINE);
    if delta == 0.0 {
        return;
    }
    let Some(p) = windows.single().ok().and_then(Window::physical_cursor_position) else { return };
    for (node, at, mut position) in &mut areas {
        if node.contains_point(*at, p) {
            let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
            position.0.y = (position.0.y - delta).clamp(0.0, max);
        }
    }
}

pub(super) fn build(app: &mut App) {
    use crate::app::{InputSet, ViewerSet};
    app.init_resource::<Drafts>()
        .add_text_field(NAME, TextField::new("Robot name").placeholder("Lift arm"))
        .add_text_field(DESCRIPTION, TextField::new("What should it do?").placeholder("A one-joint arm that lifts a 20 g weight"))
        .add_text_field(CHAT, TextField::new("Ask the design assistant").placeholder("Ask for a change"))
        .add_text_field(PATH, TextField::new("Project path").placeholder("/path/to/robot-folder"))
        .add_systems(Update, (input, scroll).in_set(InputSet::Window))
        .add_systems(Update, (strip, card).in_set(ViewerSet::Present));
}
