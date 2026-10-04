//! The controller leaderboard dialog (the browser's `web/viewer/leaderboard.js`
//! over `web/leaderboard/evaluations.json`, through the shared
//! `sim_runtime::controller_leaderboard` rule): search, status (all,
//! validated for group, experimental) and comparable-profile filters, ranks
//! only within a comparison group, compare selected, an evidence card per
//! entry (metrics, command response, gates, limitations) with Download
//! evaluation, and per row Load and run (the tested recipe opened as preset
//! `tested-<id>`, its step-0 inputs applied, then run) and Replay tested
//! inputs (the evaluated input events replayed through the shared
//! prepare_replay). Recipe and evidence files are checked against their
//! recorded sha256 on the loader thread first. Every control is a
//! `RobotAction::Leaderboard` (REST `robot_leaderboard`).
use super::*;
use crate::app::actions::Act;
use crate::jobs::{Job, Pool};
use crate::ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFieldApp, TextFocus, Typing};
use sim_runtime::controller_leaderboard as board;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// The search field.
pub(crate) const SEARCH: FieldId = FieldId("robot.leaderboard_search");

pub(super) fn add_field(app: &mut App) {
    app.add_text_field(SEARCH, TextField::new("Search controllers").placeholder("Speed, feedback, gait…"));
}

/// The status filter (the browser's select).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StatusFilter {
    #[default]
    All,
    Validated,
    Experimental,
}

/// One leaderboard intent (REST `robot_leaderboard` op).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum BoardOp {
    /// Answer the catalog (entries with rank, eligibility and gates) without changing anything.
    List,
    Open,
    Close,
    Search { text: String },
    Status { status: StatusFilter },
    /// A comparison group key (None: all profiles, no shared ranking).
    Profile { group: Option<String> },
    Select { id: String, on: bool },
    Compare,
    Inspect { id: String },
    Run { id: String },
    Replay { id: String },
    Download { id: String },
}

/// The catalog as read, with its ranks.
pub struct Loaded {
    pub catalog: board::Catalog,
    pub ranks: BTreeMap<String, usize>,
}

/// The leaderboard's state, kept across robots in the window (the dialog's filters persist as the page's do).
#[derive(Resource, Default)]
pub struct Leaderboard {
    pub open: bool,
    pub loaded: Option<Result<Arc<Loaded>, String>>,
    loading: Option<Job<Loaded>>,
    pub search: String,
    pub status: StatusFilter,
    pub profile: Option<String>,
    pub selected: BTreeSet<String>,
    /// The entries whose evidence cards are shown (one by Evidence, several by Compare).
    pub inspected: Vec<String>,
    pub notice: Option<Result<String, String>>,
    /// Bumped on every change the dialog shows (its redraw key).
    pub revision: u64,
}

impl Leaderboard {
    fn touch(&mut self) {
        self.revision += 1;
    }
    /// Starts reading the catalog (once) on `Pool::Io`.
    fn ensure_loaded(&mut self) {
        if self.loaded.is_some() || self.loading.is_some() {
            return;
        }
        let path = match crate::workspace::path(board::CATALOG) {
            Ok(p) => p,
            Err(e) => {
                self.loaded = Some(Err(format!("the leaderboard catalog resolves against the workspace root: {e}")));
                return;
            }
        };
        self.loading = Some(Job::spawn(Pool::Io, 0, "the leaderboard catalog", move |_| {
            let catalog = board::read(&path)?;
            let ranks = board::rank_entries(&catalog.entries);
            Ok(Loaded { catalog, ranks })
        }));
    }
    fn entry(&self, id: &str) -> Result<(Arc<Loaded>, Value), String> {
        let loaded = match &self.loaded {
            Some(Ok(l)) => l.clone(),
            Some(Err(e)) => return Err(format!("the leaderboard could not load: {e}")),
            None => return Err("the leaderboard is still loading (robot_leaderboard {op: open} starts it)".into()),
        };
        let e = loaded.catalog.entries.iter().find(|e| e["id"] == id).cloned().ok_or_else(|| format!("no controller evaluation `{id}` in the leaderboard"))?;
        Ok((loaded, e))
    }
    /// The entries the filters show (the browser's render), in catalog order.
    pub fn shown<'a>(&self, loaded: &'a Loaded) -> Vec<&'a Value> {
        let needle = self.search.to_lowercase();
        loaded.catalog.entries.iter().filter(|e| {
            format!("{} {}", e["name"].as_str().unwrap_or(""), e["description"].as_str().unwrap_or("")).to_lowercase().contains(&needle)
                && self.profile.as_deref().is_none_or(|g| e["comparison_group"] == g)
                && match self.status {
                    StatusFilter::All => true,
                    StatusFilter::Validated => board::eligible(e),
                    StatusFilter::Experimental => !board::eligible(e),
                }
        }).collect()
    }
    /// The comparable profiles: group key → label (fidelity · environment · simulated s · key prefix).
    pub fn groups(loaded: &Loaded) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for e in &loaded.catalog.entries {
            let key = e["comparison_group"].as_str().unwrap_or("").to_string();
            if out.iter().any(|(k, _)| *k == key) {
                continue;
            }
            let label = format!("{} · {} · {}s · {}", e["fidelity_label"].as_str().unwrap_or("?"), e["environment_label"].as_str().unwrap_or("?"), e["metrics"]["simulated_s"], key.get(..6).unwrap_or(&key));
            out.push((key, label));
        }
        out
    }
}

/// JobResults: the catalog read.
pub(super) fn receive(mut board: ResMut<Leaderboard>) {
    let Some(job) = board.loading.as_ref() else { return };
    let Some(result) = job.poll() else { return };
    board.loading = None;
    board.loaded = Some(result.map(Arc::new));
    board.touch();
}

/// `robot_state.leaderboard` and the REST `list` answer.
pub(super) fn json(board: &Leaderboard, full: bool) -> Value {
    let loaded = board.loaded.as_ref().and_then(|l| l.as_ref().ok());
    let rows: Vec<Value> = loaded.map(|l| board.shown(l).into_iter().map(|e| row_json(l, e, full)).collect()).unwrap_or_default();
    json!({"open": board.open, "loading": board.loading.is_some(), "error": board.loaded.as_ref().and_then(|l| l.as_ref().err()),
        "catalog": board::CATALOG, "scope": loaded.map(|l| &l.catalog.scope), "entries": loaded.map(|l| l.catalog.entries.len()),
        "validated": loaded.map(|l| l.catalog.entries.iter().filter(|e| board::eligible(e)).count()),
        "search": board.search, "status": board.status, "profile": board.profile, "selected": board.selected, "inspected": board.inspected,
        "groups": loaded.map(|l| Leaderboard::groups(l).into_iter().map(|(k, v)| json!({"group": k, "label": v})).collect::<Vec<_>>()),
        "shown": rows, "notice": board.notice.as_ref().map(|n| match n { Ok(m) => json!({"ok": m}), Err(e) => json!({"error": e}) }),
        "eligible_rule": board::ELIGIBLE_RULE, "required_gates": board::REQUIRED_GATES})
}
fn row_json(l: &Loaded, e: &Value, full: bool) -> Value {
    let id = e["id"].as_str().unwrap_or("");
    let gates: serde_json::Map<String, Value> = board::REQUIRED_GATES.iter().map(|g| (g.to_string(), e["gates"][*g].clone())).collect();
    let mut v = json!({"id": id, "name": e["name"], "description": e["description"], "rank": l.ranks.get(id), "eligible": board::eligible(e), "comparison_group": e["comparison_group"],
        "fidelity_label": e["fidelity_label"], "metrics": e["metrics"], "gates": gates, "preset_id": format!("tested-{id}")});
    if full {
        v["entry"] = e.clone();
    }
    v
}

fn value(v: &Value, scale: f64, digits: usize, unit: &str) -> String {
    v.as_f64().filter(|x| x.is_finite()).map_or("Not measured".into(), |x| format!("{:.*}{unit}", digits, x * scale))
}

/// The evidence card's lines (the browser's `inspect`).
pub(super) fn card(e: &Value) -> Vec<(String, String)> {
    let m = &e["metrics"];
    let host = &e["browser_host"];
    let runtime = &e["browser_runtime"];
    vec![
        ("Sustained walking".into(), value(&m["sustained_speed_m_s"], 1000.0, 3, " mm/s")),
        ("Measured window".into(), value(&m["speed_window_s"], 1.0, 2, " s")),
        ("Travel in measured window".into(), value(&m["short_speed_m_s"], 1000.0, 3, " mm/s")),
        ("Heading error at stop".into(), m["final_heading_error_rad"].as_f64().map_or("Not measured".into(), |x| format!("{:.3}°", x.to_degrees()))),
        ("Final position error".into(), value(&m["final_position_error_m"], 1000.0, 3, " mm")),
        ("Physical stop latency".into(), "Not established by endpoint error".into()),
        ("Positive shaft work".into(), value(&m["positive_mechanical_work_j"], 1.0, 3, " J (sampled)")),
        ("Native compute throughput".into(), value(&m["native_throughput"], 1.0, 2, "× (hardware not recorded)")),
        ("Active browser throughput".into(), value(&m["browser_active_throughput"], 1.0, 3, "×")),
        ("Active browser p95".into(), value(&m["browser_active_p95_s"], 1000.0, 2, " ms")),
        ("Browser hardware".into(), if host.is_object() { format!("{}; {}; browser {}", host["cpu"].as_str().unwrap_or("?"), host["platform"].as_str().unwrap_or("?"), host["browser"].as_str().unwrap_or("?")) } else { "Not recorded".into() }),
        ("Browser build".into(), if runtime.is_object() { format!("{}; WASM {}", runtime["compiler_profile"].as_str().unwrap_or("?"), runtime["browser_module_sha256"].as_str().unwrap_or("?")) } else { "Compiler settings not recorded with the browser measurement".into() }),
        ("Controller version".into(), e["controller_sha256"].as_str().unwrap_or("").into()),
        ("CAD model".into(), e["cad_sha256"].as_str().unwrap_or("").into()),
        ("Environment version".into(), e["environment_sha256"].as_str().unwrap_or("").into()),
        ("Benchmark".into(), e["benchmark_version"].to_string().trim_matches('"').into()),
        ("Seed".into(), e["load"]["seed"].to_string()),
    ]
}

/// What a tested recipe's open does once it has loaded (`scene::receive`).
pub(crate) enum AfterOpen {
    /// Apply the evaluated run's step-0 inputs, then Run.
    Run { initial: Option<Vec<f64>> },
    /// Replay the recording the loader wrote.
    Replay { path: std::path::PathBuf },
}

/// Opens tested recipe `id` (`tested-<id>` or the entry id) as a preset
/// (`board::tested_preset`): the loader thread checks the recipe and evidence
/// sha256 (`board::verify_sources`), parses the files and, for a replay,
/// writes the evaluated input events as an environment recording under
/// runs/robot-presets/tested-<id>/.
pub(crate) fn open_tested(presets: &Path, entry: &Value, then: Then) -> Result<RobotView, String> {
    let replay = then == Then::Replay;
    let id = entry["id"].as_str().unwrap_or("").to_string();
    let root = crate::workspace::root().map_err(|e| format!("tested recipe `{id}` resolves its files against the workspace root: {e}"))?.to_path_buf();
    let preset_entry = board::tested_preset(entry);
    let preset = Preset {
        id: preset_entry["id"].as_str().unwrap_or("").to_string(),
        mode: "embedded".into(),
        label: preset_entry["label"].as_str().unwrap_or("").to_string(),
        scene: preset_entry["scene"].as_str().map(str::to_string),
        config: preset_entry["config"].as_str().map(str::to_string),
        task: preset_entry["task"].as_str().map(str::to_string),
        capture: None,
        entry: preset_entry,
    };
    preset.openable(&root).map_err(|why| format!("tested recipe `{id}` cannot be opened under {}: {why}", root.display()))?;
    let replay_path = replay.then(|| root.join(crate::robot::recording::DIR).join(&preset.id).join(format!("leaderboard-replay-{}.json", crate::robot::recording::stamp(crate::robot::recording::now_ms()))));
    let (worker, dir, e, target) = (preset.clone(), root.clone(), entry.clone(), replay_path.clone());
    let path = root.join(preset.scene.as_deref().unwrap_or_default());
    let load = Job::spawn(Pool::Compute, 0, format!("{}: the tested-recipe loader", path.display()), move |_| {
        board::verify_sources(&dir, &e)?;
        let (loaded, run) = load_preset(worker, &dir)?;
        if let Some(target) = &target {
            let config = run.config.clone().ok_or("a tested recipe declares a config")?;
            let task = run.task.clone().ok_or("a tested recipe declares a task")?;
            let recording = board::replay_recording(&e, run.scene.clone(), config, task)?;
            let text = serde_json::to_string(&recording).map_err(|e| format!("serialising the evaluated inputs: {e}"))?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(target).map_err(|e| format!("{}: {e} (never overwritten)", target.display()))?;
            std::io::Write::write_all(&mut f, text.as_bytes()).map_err(|e| format!("writing {}: {e}", target.display()))?;
        }
        Ok((loaded, Some(Opened::Preset(run))))
    });
    let mut view = RobotView::new(path, Some(load), Some(preset));
    view.presets = Ok(presets.to_path_buf());
    view.section = Section::Run;
    view.after_open = match (then, replay_path) {
        (Then::Replay, Some(path)) => Some(AfterOpen::Replay { path }),
        (Then::Run, _) => Some(AfterOpen::Run { initial: board::initial_inputs(entry) }),
        _ => None,
    };
    Ok(view)
}

/// What opening a tested recipe goes on to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Then {
    /// Only open it (REST `robot_preset {id: tested-…}`).
    Open,
    /// Load and run: the step-0 inputs, then Run.
    Run,
    /// Replay tested inputs.
    Replay,
}

/// The catalog entry a `tested-<id>` preset id names (read here: a small file).
pub(crate) fn tested_entry(id: &str) -> Result<Value, String> {
    let key = id.strip_prefix("tested-").unwrap_or(id);
    let catalog = board::read(&crate::workspace::path(board::CATALOG)?)?;
    catalog.entries.into_iter().find(|e| e["id"] == key).ok_or_else(|| format!("no controller evaluation `{key}` in {}", board::CATALOG))
}

/// One leaderboard op (from `actions::apply`): Ok(Some(view)) when it opens a tested recipe in place of the current robot.
pub(super) fn handle(op: &BoardOp, b: &mut Leaderboard, view: &RobotView) -> Result<(Value, Option<RobotView>), String> {
    let mut next = None;
    match op {
        BoardOp::List => {
            b.ensure_loaded();
            return Ok((json(b, true), None));
        }
        BoardOp::Open => {
            b.open = true;
            b.ensure_loaded();
        }
        BoardOp::Close => b.open = false,
        BoardOp::Search { text } => b.search = text.clone(),
        BoardOp::Status { status } => b.status = *status,
        BoardOp::Profile { group } => b.profile = group.clone(),
        BoardOp::Select { id, on } => {
            b.entry(id)?;
            if *on {
                b.selected.insert(id.clone());
            } else {
                b.selected.remove(id);
            }
        }
        BoardOp::Compare => {
            if b.selected.len() < 2 {
                return Err("compare needs at least two selected controllers".into());
            }
            b.inspected = b.selected.iter().cloned().collect();
        }
        BoardOp::Inspect { id } => {
            b.entry(id)?;
            b.inspected = vec![id.clone()];
        }
        BoardOp::Run { id } | BoardOp::Replay { id } => {
            let (_, e) = b.entry(id)?;
            let presets = view.presets.clone()?;
            next = Some(open_tested(&presets, &e, if matches!(op, BoardOp::Replay { .. }) { Then::Replay } else { Then::Run })?);
            // As the browser's dialog: running a recipe closes it.
            b.open = false;
        }
        BoardOp::Download { id } => {
            let (_, e) = b.entry(id)?;
            let root = crate::workspace::root()?.to_path_buf();
            let dir = root.join("runs/leaderboard");
            std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
            let mut path = dir.join(format!("{id}.evaluation.json"));
            if path.exists() {
                path = dir.join(format!("{id}-{}.evaluation.json", crate::robot::recording::stamp(crate::robot::recording::now_ms())));
            }
            let text = serde_json::to_string_pretty(&e).map_err(|err| err.to_string())?;
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|err| format!("{}: {err} (never overwritten)", path.display()))?;
            std::io::Write::write_all(&mut f, text.as_bytes()).map_err(|err| format!("writing {}: {err}", path.display()))?;
            b.notice = Some(Ok(format!("evaluation written to {}", path.display())));
        }
    }
    b.touch();
    Ok((json(b, false), next))
}

// ---- The dialog -------------------------------------------------------------

#[derive(Component)]
pub(super) struct BoardRoot;
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct BoardSearchField;

/// Input: the search field (its typing becomes `Search {text}`).
pub(super) fn search_input(presses: Query<&BoardSearchField, With<crate::ui_kit::activation::Activated>>, mut msgs: MessageReader<FieldMsg>, mut text: TextFocus, b: Res<Leaderboard>, mut out: MessageWriter<Act<RobotAction>>) {
    for m in msgs.read().filter(|m| m.field == SEARCH) {
        match &m.event {
            FieldEvent::Changed(d) if d.text != b.search => {
                out.write(Act::ui(RobotAction::Leaderboard { op: BoardOp::Search { text: d.text.clone() } }));
            }
            FieldEvent::Submit(_) | FieldEvent::Cancel => text.blur(SEARCH),
            _ => {}
        }
    }
    if !presses.is_empty() && !text.focused(SEARCH) {
        text.focus_draft(SEARCH, TextDraft::new(b.search.clone(), false));
    }
}

/// Present: the dialog over the 3D view, rebuilt when the board changes (or its search field's focus).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw(mut commands: Commands, b: Res<Leaderboard>, typing: Typing, fonts: Res<UiFonts>, roots: Query<Entity, With<BoardRoot>>, mut last: Local<Option<(u64, bool, bool)>>) {
    let focused = typing.focused(SEARCH);
    let key = (b.revision, b.open, focused);
    if last.as_ref() == Some(&key) && roots.iter().next().is_some() == b.open {
        return;
    }
    *last = Some(key);
    for r in &roots {
        commands.entity(r).despawn();
    }
    if !b.open {
        return;
    }
    let k = Kit { f: &fonts };
    let op = |op: BoardOp| RobotAction::Leaderboard { op };
    let root = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(LEFT), right: Val::Px(RIGHT), top: Val::Px(TOP), bottom: Val::Px(crate::ui_kit::SWITCHER_STRIP), padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() },
            BackgroundColor(crate::ui_kit::SURFACE),
            ZIndex(20),
            BoardRoot,
            DespawnOnExit(ModeScope::Robot),
        ))
        .id();
    let mut head = vec![
        commands.spawn(Node { flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, flex_shrink: 0.0, ..default() }).with_children(|r| {
            r.spawn(k.title("Controller leaderboard"));
            r.spawn(k.button("Close", op(BoardOp::Close), Look::Secondary, true));
        }).id(),
    ];
    let loaded = match &b.loaded {
        None => {
            head.push(commands.spawn(k.text("Loading the controller evaluations…", size::BODY, SUBTLE, 0)).id());
            commands.entity(root).add_children(&head);
            return;
        }
        Some(Err(e)) => {
            head.push(commands.spawn(k.text(format!("Leaderboard unavailable: {e}"), size::BODY, DANGER, 0)).id());
            commands.entity(root).add_children(&head);
            return;
        }
        Some(Ok(l)) => l.clone(),
    };
    let entries = &loaded.catalog.entries;
    head.push(commands.spawn(k.text(format!("{} reproducible recipes · {} meet every declared gate. Speed ranks are assigned only within identical model, environment, fidelity and benchmark groups.", entries.len(), entries.iter().filter(|e| board::eligible(e)).count()), size::CAPTION, SUBTLE, 0)).id());
    if let Some(n) = &b.notice {
        let (t, c) = match n { Ok(m) => (m.clone(), TEXT), Err(e) => (e.clone(), DANGER) };
        head.push(commands.spawn(k.text(t, size::CAPTION, c, 0)).id());
    }
    // Filters: search, status, profile, compare.
    let filters = commands.spawn(wrap()).id();
    let search = commands.spawn(k.input(&b.search, "Speed, feedback, gait…", BoardSearchField, focused)).insert(AccessibleLabel::new("Search controllers")).id();
    commands.entity(filters).add_child(search);
    for (s, label) in [(StatusFilter::All, "All controllers"), (StatusFilter::Validated, "Validated for group"), (StatusFilter::Experimental, "Experimental")] {
        let c = commands.spawn(k.chip(label, op(BoardOp::Status { status: s }), b.status == s, true)).id();
        commands.entity(filters).add_child(c);
    }
    let compare = commands.spawn(k.button("Compare selected", op(BoardOp::Compare), Look::Secondary, b.selected.len() >= 2)).id();
    commands.entity(filters).add_child(compare);
    head.push(filters);
    let profiles = commands.spawn(wrap()).id();
    let all = commands.spawn(k.chip("All profiles · no shared ranking", op(BoardOp::Profile { group: None }), b.profile.is_none(), true)).id();
    commands.entity(profiles).add_child(all);
    for (key, label) in Leaderboard::groups(&loaded) {
        let on = b.profile.as_deref() == Some(key.as_str());
        let c = commands.spawn(k.chip(&clip(&label, 60), op(BoardOp::Profile { group: Some(key) }), on, true)).id();
        commands.entity(profiles).add_child(c);
    }
    head.push(profiles);
    commands.entity(root).add_children(&head);
    // Rows and evidence cards in one scroll area.
    let scroll = commands.spawn((k.scroll_area(Node { flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }, 0.0), BoardScroll)).id();
    let mut rows = Vec::new();
    if !b.inspected.is_empty() {
        let groups: BTreeSet<&str> = b.inspected.iter().filter_map(|id| entries.iter().find(|e| e["id"] == id.as_str())).filter_map(|e| e["comparison_group"].as_str()).collect();
        if groups.len() > 1 {
            rows.push(commands.spawn(k.text("Different comparison groups: inspect these results side by side, but do not interpret them as a speed ranking.", size::CAPTION, WARN, 0)).id());
        }
        let cards = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(12.0), flex_wrap: FlexWrap::Wrap, flex_shrink: 0.0, ..default() }).id();
        for id in &b.inspected {
            let Some(e) = entries.iter().find(|e| e["id"] == id.as_str()) else { continue };
            let card_node = commands.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), width: Val::Px(360.0), padding: UiRect::all(Val::Px(8.0)), border: UiRect::all(Val::Px(1.0)), ..default() }).insert(BorderColor::all(crate::ui_kit::BORDER)).id();
            let mut lines = vec![commands.spawn(k.text(e["name"].as_str().unwrap_or(""), size::ITEM, TEXT, 1)).id(), commands.spawn(k.text(e["description"].as_str().unwrap_or(""), size::CAPTION, SUBTLE, 0)).id()];
            for (name, v) in card(e) {
                lines.push(commands.spawn(k.text(format!("{name}: {v}"), size::DETAIL, TEXT, 0)).id());
            }
            if let Some(r) = e.get("command_response").filter(|r| r.is_object()) {
                lines.push(commands.spawn(k.text(format!("Measured command response — {}", r["method"].as_str().unwrap_or("")), size::CAPTION, TEXT, 1)).id());
                for c in r["cases"].as_array().into_iter().flatten() {
                    let t = if c["simulated_response_s"].is_null() { "No sustained directed response in the test window".to_string() } else { format!("{} · {}", value(&c["simulated_response_s"], 1.0, 2, " s simulation"), value(&c["drawn_wall_s"], 1.0, 2, " s to draw")) };
                    lines.push(commands.spawn(k.text(format!("{}: {t}", c["command"].as_str().unwrap_or("")), size::DETAIL, TEXT, 0)).id());
                }
                lines.push(commands.spawn(k.text(r["scope"].as_str().unwrap_or(""), size::DETAIL, SUBTLE, 0)).id());
            }
            for (g, label) in board::GATE_LABELS {
                let gate = &e["gates"][g];
                let status = gate["status"].as_str().unwrap_or("missing");
                let color = match status { "pass" => ACCENT, "fail" => DANGER, _ => WARN };
                lines.push(commands.spawn(k.text(format!("{label}: {status} — {}", gate["detail"].as_str().unwrap_or("")), size::DETAIL, color, 0)).id());
            }
            lines.push(commands.spawn(k.text(e["limitations"].as_str().unwrap_or(""), size::DETAIL, SUBTLE, 0)).id());
            lines.push(commands.spawn(k.button("Download evaluation", op(BoardOp::Download { id: id.clone() }), Look::Secondary, true)).id());
            commands.entity(card_node).add_children(&lines);
            commands.entity(cards).add_child(card_node);
        }
        rows.push(cards);
    }
    let shown = b.shown(&loaded);
    if shown.is_empty() {
        rows.push(commands.spawn(k.text("No controller matches these filters.", size::BODY, SUBTLE, 0)).id());
    }
    for e in shown {
        let id = e["id"].as_str().unwrap_or("").to_string();
        let m = &e["metrics"];
        let sustained = !m["sustained_speed_m_s"].is_null();
        let speed = if sustained { &m["sustained_speed_m_s"] } else { &m["short_speed_m_s"] };
        let rank = loaded.ranks.get(&id).map_or("—".to_string(), |r| r.to_string());
        let summary = format!(
            "#{rank} · {} · {} ({}) · {} · {}/{} swings · stop heading {} · browser {} / {} · {}",
            value(speed, 1000.0, 3, " mm/s"), if sustained { "sustained window" } else { "short window only" }, m["simulated_s"],
            if m["task_passed"] == true { "Pass" } else { "Fail" }, m["qualified_swings"], m["swings"],
            m["final_heading_error_rad"].as_f64().map_or("Not measured".into(), |x| format!("{:.3}°", x.to_degrees())),
            value(&m["browser_active_throughput"], 1.0, 3, "×"), value(&m["browser_active_p95_s"], 1000.0, 1, " ms"),
            if board::eligible(e) { "Validated for group" } else { "Experimental" });
        let row = commands.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), flex_shrink: 0.0, ..default() }).id();
        let title = commands.spawn(k.text(format!("{} — {}", e["name"].as_str().unwrap_or(""), e["fidelity_label"].as_str().unwrap_or("")), size::ITEM, TEXT, 1)).id();
        let desc = commands.spawn(k.text(e["description"].as_str().unwrap_or(""), size::CAPTION, SUBTLE, 0)).id();
        let line = commands.spawn(k.text(summary, size::DETAIL, TEXT, 0)).id();
        let actions = commands.spawn(wrap()).id();
        let selected = b.selected.contains(&id);
        let chip = commands.spawn(k.chip("Compare", op(BoardOp::Select { id: id.clone(), on: !selected }), selected, true)).id();
        let run = commands.spawn(k.button("Load and run", op(BoardOp::Run { id: id.clone() }), Look::Primary, true)).id();
        let replay = commands.spawn(k.button("Replay tested inputs", op(BoardOp::Replay { id: id.clone() }), Look::Secondary, true)).id();
        let evidence = commands.spawn(k.button("Evidence", op(BoardOp::Inspect { id: id.clone() }), Look::Secondary, true)).id();
        commands.entity(actions).add_children(&[chip, run, replay, evidence]);
        commands.entity(row).add_children(&[title, desc, line, actions]);
        rows.push(row);
    }
    commands.entity(scroll).add_children(&rows);
    commands.entity(root).add_child(scroll);
}

/// The dialog's scroll area.
#[derive(Component)]
pub(super) struct BoardScroll;

/// The wheel over the dialog scrolls it.
pub(super) fn scroll(mut wheel: MessageReader<MouseWheel>, windows: Query<&Window, With<bevy::window::PrimaryWindow>>, mut areas: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform, &mut ScrollPosition), With<BoardScroll>>) {
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
