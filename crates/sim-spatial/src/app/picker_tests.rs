//! The document picker and the recent documents without a window
//! (MinimalPlugins + StatesPlugin, as `app::tests`): a choice is the switch
//! `viewer_mode` builds, a click to a mode with no document opens the
//! picker while REST is refused, the refusals name no REST payload, the
//! recent values round-trip, and discovery walks an isolated temp tree.
use super::actions::{Act, Origin, Replies, Reply};
use super::picker::{Choice, Picker, Section, Sources, discover};
use super::recent::{KEEP, Recents};
use super::switch::{Document, ModeSwitch, Switcher, WindowAction};
use super::*;
use bevy::state::app::StatesPlugin;
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn args(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => panic!("not an object"),
    }
}

fn from_args(v: Value) -> ModeSwitch {
    ModeSwitch::from_args(args(v)).unwrap()
}

/// A fresh temp directory for one test.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("picker-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn choice(label: &str, enabled: bool, document: Document) -> Choice {
    Choice { label: label.into(), detail: if enabled { String::new() } else { "missing".into() }, enabled, document }
}

/// A picker open for `mode` with one section of `choices`.
fn picker_with(mode: ViewerMode, choices: Vec<Choice>) -> Picker {
    let mut picker = Picker::default();
    picker.open = Some(mode);
    picker.found = Some(Sources { sections: vec![Section { title: "Test".into(), empty: String::new(), choices }], start_dir: None });
    picker
}

/// (i) Every choice is the exact request `viewer_mode {mode, path | preset | url}` builds.
#[test]
fn a_picker_choice_is_the_switch_viewer_mode_builds() {
    let picker = picker_with(
        ViewerMode::Robot,
        vec![
            choice("Measured", true, Document::Preset("robot-measured-400hz".into())),
            choice("x", true, Document::Path("/abs/x.simrobot.json".into())),
            choice("gone", false, Document::Path("/abs/gone.simrobot.json".into())),
        ],
    );
    assert_eq!(picker.choice(0, 0), Some(from_args(json!({"mode": "robot", "preset": "robot-measured-400hz"}))));
    assert_eq!(picker.choice(0, 1), Some(from_args(json!({"mode": "robot", "path": "/abs/x.simrobot.json"}))));
    assert_eq!(picker.choice(0, 2), None, "a disabled entry asks for nothing");
    assert_eq!(picker.choice(1, 0), None);

    let mut picker = picker;
    assert_eq!(picker.typed(), None, "nothing typed");
    picker.draft.text = " /abs/y.simrobot.json ".into();
    assert_eq!(picker.typed(), Some(from_args(json!({"mode": "robot", "path": "/abs/y.simrobot.json"}))));

    let mut cad = picker_with(ViewerMode::Cad, vec![choice("svc", true, Document::Url("http://127.0.0.1:8420".into()))]);
    assert_eq!(cad.choice(0, 0), Some(from_args(json!({"mode": "cad", "url": "http://127.0.0.1:8420"}))));
    cad.draft.text = "http://127.0.0.1:9000".into();
    assert_eq!(cad.typed(), Some(from_args(json!({"mode": "cad", "url": "http://127.0.0.1:9000"}))));
    cad.draft.text = "/abs/m.rcad".into();
    assert_eq!(cad.typed(), Some(from_args(json!({"mode": "cad", "path": "/abs/m.rcad"}))));
    // A URL is a path in any other mode.
    let mut build = picker_with(ViewerMode::Build, Vec::new());
    build.draft.text = "http://x".into();
    assert_eq!(build.typed().and_then(|s| s.document), Some(Document::Path("http://x".into())));

    // The controls' actions are the same requests, by flat index.
    let controls = picker_with(ViewerMode::Robot, vec![choice("Measured", true, Document::Preset("p".into())), choice("x", true, Document::Path("/abs/x.simrobot.json".into()))]).controls();
    let ids: Vec<&str> = controls.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["picker:robot:0", "picker:robot:1", "picker:path", "picker:close"]);
    assert_eq!(from_args(controls[0]["action"]["viewer_mode"].clone()), ModeSwitch { mode: ViewerMode::Robot, document: Some(Document::Preset("p".into())), reveal: None });
    assert_eq!(from_args(controls[1]["action"]["viewer_mode"].clone()), ModeSwitch { mode: ViewerMode::Robot, document: Some(Document::Path("/abs/x.simrobot.json".into())), reveal: None });
    assert_eq!(controls[0]["label"], "Open Measured");
}

fn mode(app: &App) -> ViewerMode {
    *app.world().resource::<State<ViewerMode>>().get()
}

/// The window's switch on MinimalPlugins, in inspect mode with no other document.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin)).insert_resource(crate::models::ModelLibrary::default()).add_plugins(ModesPlugin { initial: ViewerMode::Inspect });
    app.update();
    app
}

fn rest(app: &mut App, action: WindowAction) -> Reply {
    let reply = app.world_mut().resource_mut::<Replies>().open();
    app.world_mut().write_message(Act { action, origin: Origin::Rest(reply) });
    reply
}

fn settle(app: &mut App, reply: Reply) -> Result<Value, String> {
    for _ in 0..500 {
        app.update();
        if let Some(outcome) = app.world_mut().resource_mut::<Replies>().take(reply) {
            return match outcome {
                sim_api::Outcome::Done(result) => result,
                _ => panic!("a window action answers Done"),
            };
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("{reply:?} did not settle");
}

fn activate(id: &str, text: Option<&str>) -> WindowAction {
    let mut action = json!({"operation": "activate", "id": id, "ui_revision": 0});
    if let Some(text) = text {
        action["text"] = json!(text);
    }
    WindowAction::SystemUi(args(json!({"action": action})))
}

/// (ii) A click on Robot with no robot opens the picker and keeps the mode;
/// REST `viewer_mode {"mode":"robot"}` is still refused; a refused choice
/// keeps the picker open; Close closes it.
#[test]
fn a_click_to_a_mode_with_no_document_opens_the_picker_and_rest_is_refused() {
    let mut app = app();
    app.world_mut().write_message(Act::ui(WindowAction::Switch(ModeSwitch { mode: ViewerMode::Robot, document: None, reveal: None })));
    app.update();
    assert_eq!(app.world().resource::<Picker>().open, Some(ViewerMode::Robot));
    assert_eq!(mode(&app), ViewerMode::Inspect, "the current mode stays");
    assert!(matches!(&app.world().resource::<Switcher>().message, Some(Ok(m)) if m.contains("Choose a robot for Robot mode in the picker")));

    let reply = rest(&mut app, WindowAction::ViewerMode(args(json!({"mode": "robot"}))));
    let e = settle(&mut app, reply).unwrap_err();
    assert!(e.contains("robot mode needs a robot") && e.contains("Inspect mode stays") && !e.contains("viewer_mode"), "{e}");
    assert_eq!(mode(&app), ViewerMode::Inspect);

    // A system_ui mode:<mode> activation is interactive: it opens the picker and says so.
    let reply = rest(&mut app, activate("mode:place", None));
    let opened = settle(&mut app, reply).unwrap();
    assert_eq!(opened["picker"], "place");
    assert!(opened["controls"].as_array().unwrap().iter().any(|c| c["id"] == "picker:close"), "{opened}");
    assert_eq!(app.world().resource::<Picker>().open, Some(ViewerMode::Place));

    // An interactive switch to CAD with no document opens the picker (REST falls back to the default URL).
    app.world_mut().write_message(Act::ui(WindowAction::Switch(ModeSwitch { mode: ViewerMode::Cad, document: None, reveal: None })));
    app.update();
    assert_eq!(app.world().resource::<Picker>().open, Some(ViewerMode::Cad));
    assert_eq!(mode(&app), ViewerMode::Inspect);

    // A choice that is refused keeps the picker open, its refusal the switcher's line.
    app.world_mut().write_message(Act::ui(WindowAction::Switch(ModeSwitch { mode: ViewerMode::Robot, document: None, reveal: None })));
    app.update();
    let missing = "/no/such/dir/robot.simrobot.json";
    let reply = rest(&mut app, activate("picker:path", Some(missing)));
    let e = settle(&mut app, reply).unwrap_err();
    assert!(e.contains("no such file") && e.contains(missing), "{e}");
    assert_eq!(app.world().resource::<Picker>().open, Some(ViewerMode::Robot), "a refused choice keeps the picker open");
    assert_eq!(app.world().resource::<Picker>().draft.text, missing);
    let (revision, opened) = (app.world().resource::<Switcher>().revision, app.world().resource::<Picker>().opened_revision);
    assert!(revision > opened, "the refusal is newer than the picker, so it is its status line");

    // Another mode's index is refused by name; Close closes it.
    let reply = rest(&mut app, activate("picker:cad:0", None));
    assert!(settle(&mut app, reply).unwrap_err().contains("robot"));
    let reply = rest(&mut app, activate("picker:close", None));
    assert_eq!(settle(&mut app, reply).unwrap()["closed"], "robot");
    assert_eq!(app.world().resource::<Picker>().open, None);
    let reply = rest(&mut app, activate("picker:close", None));
    assert!(settle(&mut app, reply).unwrap_err().contains("no document picker is open"));
}

/// (iii) The refusals a REST caller gets (and the switcher shows) name the
/// window's way, never a REST payload or command.
#[test]
fn refusals_name_no_rest_payload() {
    let mut app = app();
    let cases = [
        (ViewerMode::Build, None),
        (ViewerMode::Lessons, None),
        (ViewerMode::Robot, None),
        (ViewerMode::Place, None),
        (ViewerMode::Phenomena, Some(Document::Path("/abs/x.system.json".into()))),
        (ViewerMode::Inspect, Some(Document::Path("/abs/a.description.json".into()))),
    ];
    for (target, document) in cases {
        let reply = rest(&mut app, WindowAction::Switch(ModeSwitch { mode: target, document, reveal: None }));
        let e = settle(&mut app, reply).unwrap_err();
        assert!(e.starts_with("Not switching to") && e.contains("Inspect mode stays"), "{target:?}: {e}");
        for word in ["viewer_mode", "{", "REST", "system_open", "lesson_open", "robot_preset", "cad_open", "phenomena_select"] {
            assert!(!e.contains(word), "{target:?} names {word}: {e}");
        }
        assert_eq!(mode(&app), ViewerMode::Inspect);
    }
    assert!(app.world().resource::<Picker>().open.is_none(), "REST never opens the picker");
}

/// Value semantics remain independent of the owner's persistence seam.
/// Migration, malformed input and publication failures live in settings fixtures.
#[test]
fn recents_round_trip_dedupe_and_limit() {
    let mut recents = Recents::default();
    let a = Document::Path("/isolated/a.simrobot.json".into());
    recents.record(ViewerMode::Robot, &a, 1);
    recents.record(ViewerMode::Robot, &Document::Preset("p".into()), 2);
    recents.record(ViewerMode::Robot, &a, 3);
    recents.record(ViewerMode::Cad, &Document::Url("http://127.0.0.1:8420".into()), 4);
    let loaded: Recents = serde_json::from_slice(&serde_json::to_vec(&recents).unwrap()).unwrap();
    assert_eq!(loaded.list(ViewerMode::Robot), vec![a, Document::Preset("p".into())]);
    assert_eq!(loaded.modes["robot"][0].opened, 3);
    assert_eq!(loaded.list(ViewerMode::Cad), vec![Document::Url("http://127.0.0.1:8420".into())]);
    assert!(loaded.list(ViewerMode::Build).is_empty());
    for i in 0..KEEP + 2 {
        recents.record(ViewerMode::Build, &Document::Preset(format!("s{i}")), 10 + i as u64);
    }
    let build = recents.list(ViewerMode::Build);
    assert_eq!(build.len(), KEEP);
    assert_eq!(build[0], Document::Preset(format!("s{}", KEEP + 1)));
}

fn section<'a>(sources: &'a Sources, title: &str) -> &'a Section {
    sources.sections.iter().find(|s| s.title == title).unwrap_or_else(|| panic!("no section {title}: {sources:?}"))
}

/// (v) Discovery against a temp tree: example files by suffix (never under
/// `runs/`), place folders, recent documents (a missing one disabled).
#[test]
fn discovery_finds_examples_and_skips_runs() {
    let root = temp("discover");
    for (file, text) in [("examples/a/x.simrobot.json", "{}"), ("examples/runs/skip.simrobot.json", "{}"), ("examples/.hidden/h.simrobot.json", "{}"), ("examples/b/place/place.json", "{}"), ("lessons/one/lesson.md", "# One")] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let go = AtomicBool::new(false);
    let robot = discover(ViewerMode::Robot, Some(root.clone()), None, Recents::default(), true, &go);
    let titles: Vec<&str> = robot.sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["Recent", "Presets", "Examples"]);
    assert!(section(&robot, "Recent").choices.is_empty() && section(&robot, "Presets").choices.is_empty());
    let examples = &section(&robot, "Examples").choices;
    assert_eq!(examples.len(), 1, "{examples:?}");
    assert_eq!(examples[0].label, "examples/a/x.simrobot.json");
    assert_eq!(examples[0].document, Document::Path(root.join("examples/a/x.simrobot.json")));
    assert_eq!(robot.start_dir, Some(format!("{}/", root.join("examples").display())));

    let place = discover(ViewerMode::Place, Some(root.clone()), None, Recents::default(), true, &go);
    let places = &section(&place, "Examples").choices;
    assert_eq!(places.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["examples/b/place"]);
    assert_eq!(places[0].document, Document::Path(root.join("examples/b/place")));

    let lessons = discover(ViewerMode::Lessons, Some(root.clone()), None, Recents::default(), true, &go);
    assert_eq!(section(&lessons, "Lesson folders").choices.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["lessons"]);

    let cad = discover(ViewerMode::Cad, Some(root.clone()), None, Recents::default(), true, &go);
    assert!(cad.sections.iter().all(|section| section.choices.iter().all(|choice| !matches!(choice.document, Document::Url(_)))), "native CAD picker offers local archives, never service attachment");
    assert!(section(&cad, "Examples").choices.is_empty() && !section(&cad, "Examples").empty.is_empty());

    // Recent documents: a path that no longer exists is shown, disabled.
    let mut recents = Recents::default();
    let gone = root.join("examples/gone.simrobot.json");
    recents.record(ViewerMode::Robot, &Document::Path(gone.clone()), 1);
    recents.record(ViewerMode::Robot, &Document::Path(root.join("examples/a/x.simrobot.json")), 2);
    let robot = discover(ViewerMode::Robot, Some(root.clone()), None, recents, true, &go);
    let recent = &section(&robot, "Recent").choices;
    assert_eq!(recent.len(), 2);
    assert!(recent[0].enabled && recent[0].label == "examples/a/x.simrobot.json");
    assert!(!recent[1].enabled && recent[1].detail == "missing" && recent[1].document == Document::Path(gone));

    // No workspace root: the sections say so instead of listing nothing silently.
    let none = discover(ViewerMode::Build, None, None, Recents::default(), true, &go);
    assert!(section(&none, "Examples").empty.contains("No workspace root"));

    // A cancelled discovery stops its walk and says the section was cut.
    let cancelled = discover(ViewerMode::Robot, Some(root.clone()), None, Recents::default(), true, &AtomicBool::new(true));
    let cut = cancelled.sections.iter().find(|s| s.title.starts_with("Examples")).unwrap();
    assert_eq!(cut.title, "Examples (the search stopped early)");
    assert!(cut.choices.is_empty() && cut.empty.contains("first"), "{cut:?}");
    std::fs::remove_dir_all(&root).ok();
}

/// A `picker:<mode>:<n>` activation names the picker's listing it was read
/// from (`picker_revision`, as each entry reports it; the answer's
/// `ui_revision` is the mode's and is not checked here): one listed at another
/// revision is refused, naming both, before its index is used; the current
/// one, or none given, is applied. `picker:path` is not indexed: not checked.
#[test]
fn a_stale_picker_index_is_refused_naming_both_revisions() {
    let mut app = app();
    app.world_mut().write_message(Act::ui(WindowAction::Switch(ModeSwitch { mode: ViewerMode::Robot, document: None, reveal: None })));
    app.update();
    // Discovery lands (and moves the listing on) before the entries are read.
    for _ in 0..500 {
        if app.world().resource::<Picker>().found.is_some() {
            break;
        }
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.world().resource::<Picker>().found.is_some(), "discovery finished");
    let missing = "/no/such/dir/stale.simrobot.json";
    app.world_mut().resource_mut::<Picker>().found = Some(Sources { sections: vec![Section { title: "Test".into(), empty: String::new(), choices: vec![choice("stale", true, Document::Path(missing.into()))] }], start_dir: None });
    let controls = app.world().resource::<Picker>().controls();
    let listed = controls[0]["picker_revision"].as_u64().expect("an entry reports its picker_revision");
    assert_eq!(controls[0]["id"], "picker:robot:0");
    assert!(listed > 0 && controls.iter().filter(|c| c.get("picker_revision").is_some()).count() == 1, "only indexed entries carry it: {controls:?}");

    let at = |id: &str, ui_revision: Option<u64>| {
        let mut action = json!({"operation": "activate", "id": id});
        if let Some(r) = ui_revision {
            action["picker_revision"] = json!(r);
        }
        WindowAction::SystemUi(args(json!({"action": action})))
    };
    let stale = listed - 1;
    let reply = rest(&mut app, at("picker:robot:0", Some(stale)));
    let e = settle(&mut app, reply).unwrap_err();
    assert_eq!(e, format!("picker:robot:0 was listed at picker_revision {stale}; the picker is now at {listed}; list the controls again"));
    assert_eq!(app.world().resource::<Picker>().open, Some(ViewerMode::Robot), "a refused activation keeps the picker");

    // The current listing (or none given) reaches the switch, which refuses the missing file.
    for ui_revision in [Some(listed), None] {
        let reply = rest(&mut app, at("picker:robot:0", ui_revision));
        let e = settle(&mut app, reply).unwrap_err();
        assert!(e.contains("no such file") && e.contains(missing), "{ui_revision:?}: {e}");
    }
    // Not indexed: picker:path is applied whatever ui_revision it gives.
    let reply = rest(&mut app, WindowAction::SystemUi(args(json!({"action": {"operation": "activate", "id": "picker:path", "text": missing, "picker_revision": stale}}))));
    assert!(settle(&mut app, reply).unwrap_err().contains("no such file"));
}
