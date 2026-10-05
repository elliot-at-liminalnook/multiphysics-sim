//! Sidebar tabs: Library, Outline, References, Systems, Actuators (registry)
//! and Studies.
use super::*;

pub(super) fn library_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    // Executable controllers and generated assemblies come from files next to the system.
    for (purpose, placeholder, action) in [
        (Purpose::AddFmu, "Add an FMU block: path [period s] · Enter", BuildAction::AddFmuPath),
        (Purpose::AddRobot, "Add a robot: path to .simrobot.json · Enter", BuildAction::AddRobotPath),
    ] {
        let focused = b.input.as_ref().is_some_and(|i| i.purpose == purpose);
        let shown = b.input.as_ref().filter(|_| focused).map(|i| i.buffer.clone()).unwrap_or_default();
        body.spawn(k.input(&shown, placeholder, action, focused));
    }
    let focused = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::Filter);
    let shown = b.input.as_ref().filter(|_| focused).map(|i| i.buffer.clone()).unwrap_or_else(|| b.filter.clone());
    body.spawn(k.input(&shown, "Search components and subsystems   ( / )", BuildAction::Filter, focused));
    body.spawn(Node { margin: UiRect::vertical(Val::Px(8.)), column_gap: Val::Px(5.), row_gap: Val::Px(5.), align_items: AlignItems::Default, ..wrap() })
        .with_children(|chips| {
            chips.spawn(k.chip("All", BuildAction::Category(None), b.category.is_none(), true));
            for c in CATEGORIES {
                if b.palette.iter().any(|p| category(&p.domain) == c || (c == "Subsystems" && matches!(p.kind, InstanceKind::Subsystem { .. }))) {
                    chips.spawn(k.chip(c, BuildAction::Category(Some(c)), b.category == Some(c), true));
                }
            }
        });
    let grid=b.grid();
    body.spawn(wrap()).with_children(|r| {
        r.spawn(k.chip("Grid",BuildAction::GridVisible,grid.visible,true));
        r.spawn(k.chip("Snap",BuildAction::GridSnap,grid.snap,true));
        r.spawn(k.button(&format!("{:?}",grid.plane),BuildAction::GridPlane,Look::Secondary,true));
        r.spawn(k.button(&format!("{} mm",grid.spacing_m*1000.),BuildAction::GridSpacing,Look::Secondary,true));
        r.spawn(k.button("Origin",BuildAction::GridOrigin,Look::Secondary,true));
    });
    for purpose in [Purpose::GridSpacing,Purpose::GridOrigin] {if let Some(i)=b.input.as_ref().filter(|i|i.purpose==purpose){body.spawn(k.input(&i.buffer,"Metres · Enter to save",BuildAction::GridSpacing,true));}}
    body.spawn(k.text("Display layout only · drag a component into the grid", size::DETAIL, FAINT, 0));
    let items = b.filtered();
    body.spawn(k.text(format!("{} result{}  ·  click for details, then place or snap", items.len(), if items.len() == 1 { "" } else { "s" }), size::DETAIL, FAINT, 0));
    for (i, item) in items.iter().enumerate().take(PALETTE_ROWS) {
        let subtitle = match &item.kind {
            InstanceKind::Element { component_type } => component_type.clone(),
            InstanceKind::Subsystem { definition } => format!("{} · {definition}", if item.library_path.is_some() { "Library subsystem" } else { "Subsystem in this file" }),
            other => sim_system::kind_label(other),
        };
        let shown = b.preview.as_ref().is_some_and(|p| p.kind == item.kind);
        body.spawn(k.item(&b.icon(&item.kind), &item.label, &subtitle, category(&item.domain), BuildAction::Preview(i), shown)).observe(placement::start_palette);
    }
    if items.len() > PALETTE_ROWS {
        body.spawn(k.note(format!("{} more. Refine the search.", items.len() - PALETTE_ROWS)));
    }
}

pub(super) fn outline_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, selected: &BTreeSet<String>) {
    let definition_id = b.definition_id().unwrap_or_else(|| b.document.root.clone());
    let Some(d) = b.document.definitions.get(&definition_id) else { return };
    let shared = Resolver::new(&b.document, &b.registry).placements(&definition_id);
    body.spawn(k.text(&d.label, 15., TEXT, 2));
    body.spawn(k.text(format!("{definition_id}{}", if shared > 1 { format!("  ·  shared by {shared} placements") } else { String::new() }), size::CAPTION, SUBTLE, 0));
    if !b.level.is_empty() {
        body.spawn(Node { margin: UiRect::top(Val::Px(6.)), ..wrap() }).with_children(|r| {
            r.spawn(k.button("Up one level", BuildAction::Up, Look::Secondary, true));
        });
    }
    body.spawn(k.section(&format!("Contents  {}", d.instances.len())));
    if d.instances.is_empty() {
        body.spawn(k.text("Empty. Place components from the Library tab.", size::BODY, SUBTLE, 0));
    }
    for (name, spec) in &d.instances {
        let title = if spec.label.is_empty() { name.clone() } else { format!("{}  ", spec.label) };
        let subtitle = format!("{name}  ·  {}", kind_text(&spec.kind));
        body.spawn(k.item(&b.icon(&spec.kind), &title, &subtitle, category(domain_of(&spec.kind)), BuildAction::Select(name.clone()), selected.contains(name)));
    }
    if !d.ports.is_empty() {
        body.spawn(k.section("Boundary ports"));
        let resolver = Resolver::new(&b.document, &b.registry);
        for port in d.ports.keys() {
            let schema = resolver.boundary_schema(&definition_id, port, &mut Default::default()).ok().flatten();
            k.property(body, port, &schema.as_ref().map(sim_system::commands::describe).unwrap_or_else(|| "untyped".into()), "", None::<BuildAction>, false);
        }
    }
    if !d.nets.is_empty() {
        body.spawn(k.section(&format!("Nets  {}", d.nets.len())));
        for net in d.nets.iter().take(60) {
            let label = if net.label.is_empty() { "net".to_string() } else { net.label.clone() };
            body.spawn((
                Node { flex_direction: FlexDirection::Column, padding: UiRect::vertical(Val::Px(3.)), flex_shrink: 0., ..default() },
                children![k.text(label, size::SMALL, TEXT, 1), k.text(net.terminals.iter().map(|t| t.to_string()).collect::<Vec<_>>().join("  ·  "), size::DETAIL, SUBTLE, 0)],
            ));
        }
    }
}

pub(super) fn references_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    body.spawn(k.caption("Reference images sit on a plane in this level. They are presentation only and never affect physics."));
    let focused = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::ImportImage);
    let shown = b.input.as_ref().filter(|_| focused).map(|i| i.buffer.clone()).unwrap_or_default();
    body.spawn((Node { margin: UiRect::top(Val::Px(10.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.), flex_shrink: 0., ..default() }, children![k.input(&shown, "Paste an image path, then Enter", BuildAction::ImportImage, focused), k.text("Or drop a PNG or JPEG onto the viewport.", size::DETAIL, FAINT, 0)]));
    let Some(d) = b.definition() else { return };
    let refs: Vec<_> = d.references.iter().filter(|(_, r)| r.view == ReferenceView::Spatial).collect();
    body.spawn(k.section(&format!("On this level  {}", refs.len())));
    for (id, r) in refs {
        let calibrating = b.calibrating.as_ref().is_some_and(|(c, _)| c == id);
        body.spawn((
            Node { border_radius: BorderRadius::all(Val::Px(6.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), padding: UiRect::all(Val::Px(10.)), margin: UiRect::bottom(Val::Px(6.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor::all(if calibrating { ACCENT } else { BORDER }),
            BackgroundColor(RAISED),
        ))
        .with_children(|card| {
            card.spawn(k.text(&r.label, size::ITEM, TEXT, 1));
            let width_focus = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::ReferenceWidth(id.clone()));
            let width = b.input.as_ref().filter(|_| width_focus).map(|i| format!("{}|", i.buffer)).unwrap_or_else(|| format!("{:.3}", r.width));
            k.property(card, "Width", &width, "m", Some(BuildAction::Width(id.clone())), width_focus);
            k.property(card, "Opacity", &format!("{:.0}", r.opacity * 100.), "%", None::<BuildAction>, false);
            card.spawn(wrap()).with_children(|row| {
                row.spawn(k.button("-", BuildAction::Opacity(id.clone(), -0.1), Look::Secondary, true));
                row.spawn(k.button("+", BuildAction::Opacity(id.clone(), 0.1), Look::Secondary, true));
                row.spawn(k.button(if calibrating { "Click two points..." } else { "Calibrate" }, BuildAction::Calibrate(id.clone()), if calibrating { Look::Primary } else { Look::Secondary }, !r.locked));
                row.spawn(k.button(if r.locked { "Unlock" } else { "Lock" }, BuildAction::Lock(id.clone()), Look::Ghost, true));
                row.spawn(k.button("Remove", BuildAction::RemoveReference(id.clone()), Look::Danger, true));
            });
        });
    }
    if let Some(TextInput { purpose: Purpose::Distance { .. }, buffer }) = &b.input {
        body.spawn(k.section("Calibration"));
        body.spawn(k.input(buffer, "Real distance between the points (m)", BuildAction::Tab(Tab::References), true));
    }
}

/// Open another system file in this window: a path field and the system
/// files found under examples/systems-builder, the library and this file's folder.
pub(super) fn systems_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    body.spawn(k.caption("Open another system in this window. Its runs, notes and studies come with it; edits are already saved to this file."));
    body.spawn(k.section("Open"));
    body.spawn(k.text(&b.document.title, size::ITEM, TEXT, 1));
    body.spawn(k.text(b.path().display().to_string(), size::DETAIL, FAINT, 0));
    let focused = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::OpenSystem);
    let shown = b.input.as_ref().filter(|_| focused).map(|i| i.buffer.clone()).unwrap_or_default();
    body.spawn(Node { margin: UiRect::top(Val::Px(8.)), flex_direction: FlexDirection::Column, flex_shrink: 0., ..default() })
        .with_children(|c| {
            c.spawn(k.input(&shown, "Path to a .system.json file · Enter to open", BuildAction::OpenSystemPath, focused));
        });
    if let Some(pending) = b.open.pending() {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(ACCENT), k.text(format!("Opening {}…", pending.display()), size::SMALL, TEXT, 0)]));
        body.spawn(wrap()).with_children(|r| {
            r.spawn(k.button("Cancel", BuildAction::CancelOpen, Look::Danger, true));
        });
    }
    if let Some(e) = b.action_error.as_ref().filter(|e| e.contains("open")) {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, size::SMALL, DANGER, 0)]));
    }
    let blockers = b.open_blockers();
    if !blockers.is_empty() {
        body.spawn(k.text(format!("Before opening: {}", blockers.join("; ")), size::CAPTION, WARN, 0));
    }
    body.spawn(k.section(&format!("Systems  {}", b.open.systems.len())));
    // Shown relative to the workspace root (absolute when outside it or unresolved).
    let base = crate::workspace::root().ok();
    let open = std::path::absolute(b.path()).unwrap_or_else(|_| b.path().to_path_buf());
    for path in &b.open.systems {
        let current = *path == open;
        let name = path.file_name().map(|n| n.to_string_lossy().trim_end_matches(".system.json").to_string()).unwrap_or_default();
        let shown = base.and_then(|b| path.strip_prefix(b).ok()).unwrap_or(path).display().to_string();
        body.spawn(k.item("", &name, &shown, if current { "Open" } else { "" }, BuildAction::OpenSystem(path.clone()), current));
    }
}

/// First and last characters of a hash (the full value is in system_state).
fn short_hash(h: &str) -> String {
    if h.len() > 16 { format!("{}…{}", &h[..10], &h[h.len() - 4..]) } else { h.to_string() }
}

/// The accepted actuator registry, read-only: families with their hashes,
/// acceptance notes, limitations and every parameter's provenance and
/// uncertainty, the joint roles, and consumer-file staleness checks.
pub(super) fn actuators_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, studies: &calibration::study::StudyOwner, study_ui: &calibration::study::forms::StudyUi) {
    body.spawn(Node { margin: UiRect::bottom(Val::Px(6.)), ..wrap() }).with_children(|chips| {
        chips.spawn(k.chip("Registry", BuildAction::ActuatorView(calibration::ActuatorView::Registry), b.actuator_view == calibration::ActuatorView::Registry, true));
        chips.spawn(k.chip("Measured evidence", BuildAction::ActuatorView(calibration::ActuatorView::Evidence), b.actuator_view == calibration::ActuatorView::Evidence, true));
    });
    match b.actuator_view {
        calibration::ActuatorView::Registry => registry_view(body, k, b),
        calibration::ActuatorView::Evidence => calibration::section(body, k, b, studies, study_ui),
    }
}

fn registry_view(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    let a = &b.actuators;
    body.spawn(k.caption("The accepted actuator registry: the single source of measured motor values. Read-only here; families change only by promoting new evidence."));
    body.spawn(k.section("Registry"));
    let focused = |p: Purpose| b.input.as_ref().is_some_and(|i| i.purpose == p);
    let registry_focused = focused(Purpose::ActuatorRegistry);
    let shown = if registry_focused { b.input.as_ref().map(|i| i.buffer.clone()).unwrap_or_default() } else { a.registry.as_ref().map(|p| p.display().to_string()).unwrap_or_default() };
    body.spawn(k.input(&shown, "Path to an actuator registry.json · Enter to load", BuildAction::ActuatorRegistryPath, registry_focused));
    body.spawn(Node { margin: UiRect::top(Val::Px(6.)), ..wrap() }).with_children(|r| {
        r.spawn(k.button("Reload", BuildAction::ActuatorReload, Look::Secondary, a.pending().is_none()));
        if a.pending().is_some() {
            r.spawn(k.button("Cancel", BuildAction::CancelActuators, Look::Danger, true));
        }
    });
    if let Some(pending) = a.pending() {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(ACCENT), k.text(format!("Loading {}…", pending.display()), size::SMALL, TEXT, 0)]));
    }
    if let Some(e) = &a.error {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, size::SMALL, DANGER, 0)]));
    }
    let Some(view) = &a.shown else {
        if a.pending().is_none() && a.error.is_none() {
            body.spawn(k.caption("Not loaded yet."));
        }
        return;
    };
    let r = &view.registry;
    if a.error.is_some() {
        body.spawn(k.text(format!("Still showing the last good load: {}", r.path.display()), size::CAPTION, WARN, 1));
    }
    body.spawn(k.text(format!("Showing {}", r.path.display()), size::DETAIL, FAINT, 0));
    k.property(body, "Registry hash", &short_hash(&r.registry_hash), "", None::<BuildAction>, false);
    k.property(body, "Families", &r.families.len().to_string(), "", None::<BuildAction>, false);

    body.spawn(k.section("Consumer check"));
    let consumer_focused = focused(Purpose::ActuatorConsumer);
    let typed = if consumer_focused { b.input.as_ref().map(|i| i.buffer.clone()).unwrap_or_default() } else { String::new() };
    body.spawn(k.input(&typed, "File embedding a robot model · Enter to check", BuildAction::ActuatorConsumerPath, consumer_focused));
    if !a.check.is_empty() {
        body.spawn(Node { margin: UiRect::top(Val::Px(6.)), ..wrap() }).with_children(|r| {
            r.spawn(k.button("Check again", BuildAction::ActuatorReload, Look::Secondary, a.pending().is_none()));
        });
    }
    let base = crate::workspace::root().ok();
    for check in &view.checks {
        let current = check.is_current();
        let file = base.and_then(|b| check.file.strip_prefix(b).ok()).unwrap_or(&check.file).display().to_string();
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, margin: UiRect::top(Val::Px(8.)), flex_shrink: 0., ..default() }, children![k.dot(if current { OK } else { WARN }), k.text(format!("{} · {file}", if current { "Current" } else if check.issue.is_some() { "Not checked" } else { "Stale" }), size::SMALL, TEXT, 1)]));
        if let Some(issue) = &check.issue {
            let kind = serde_json::to_value(issue.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            body.spawn(k.text(format!("{kind}: {}", issue.message), size::CAPTION, if kind == "no_robot" { SUBTLE } else { DANGER }, 0));
        }
        for m in &check.models {
            let status = serde_json::to_value(m.status).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let pointer = if m.pointer.is_empty() { "(whole file)" } else { m.pointer.as_str() };
            body.spawn(k.text(format!("{pointer}: {status}"), size::CAPTION, if status == "current" { SUBTLE } else { WARN }, 1));
            if let Some(x) = &m.mismatch {
                let or_none = |v: &Option<String>| v.clone().unwrap_or_else(|| "none".into());
                body.spawn(k.text(format!("motor {} · joint {}", or_none(&x.motor), or_none(&x.joint)), size::DETAIL, SUBTLE, 0));
                body.spawn(k.text(format!("family {} → accepted {}", or_none(&x.family), or_none(&x.accepted_family)), size::DETAIL, SUBTLE, 0));
                body.spawn(k.text(format!("have     {}", x.have_hash.as_deref().map(short_hash).unwrap_or_else(|| "none".into())), size::DETAIL, WARN, 0));
                body.spawn(k.text(format!("accepted {}", x.accepted_hash.as_deref().map(short_hash).unwrap_or_else(|| "none".into())), size::DETAIL, OK, 0));
            } else if let Some(message) = &m.message {
                body.spawn(k.text(message, size::DETAIL, SUBTLE, 0));
            }
        }
    }

    body.spawn(k.section("Roles"));
    for (suffix, family) in &r.roles {
        k.property(body, suffix, family, "", None::<BuildAction>, false);
    }
    for f in &r.families {
        body.spawn(k.section(&format!("Family  {}", f.name)));
        k.property(body, "Content hash", &short_hash(&f.content_hash), "", None::<BuildAction>, false);
        body.spawn(k.text(format!("Accepted: {}", f.accepted), size::CAPTION, TEXT, 0));
        body.spawn(k.text(&f.description, size::DETAIL, SUBTLE, 0));
        if !f.limitations.is_empty() {
            body.spawn(k.text("Limitations", size::DETAIL, FAINT, 2));
            for l in &f.limitations {
                body.spawn(k.text(format!("· {l}"), size::DETAIL, WARN, 0));
            }
        }
        if !f.has_envelope {
            body.spawn(k.text("No measured envelope.", size::DETAIL, FAINT, 0));
        }
        let mut group = "";
        for p in &f.parameters {
            if p.group != group {
                group = p.group;
                body.spawn((k.text(group.to_uppercase(), 10., FAINT, 2), Node { margin: UiRect::top(Val::Px(6.)), ..default() }));
            }
            let uncertainty = p.uncertainty.map(|u| format!("± {}", num(u))).unwrap_or_else(|| "± unknown".into());
            let color = match p.provenance.as_str() { "measured" => OK, "derived" => ACCENT, _ => WARN };
            body.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() }).with_children(|row| {
                row.spawn(Node { justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(8.), ..default() }).with_children(|top| {
                    top.spawn(k.text(&p.name, size::SMALL, TEXT, 1));
                    top.spawn(k.text(format!("{} {}", num(p.value), p.unit), size::SMALL, TEXT, 0));
                });
                row.spawn(Node { column_gap: Val::Px(6.), ..default() }).with_children(|bottom| {
                    bottom.spawn(k.text(&p.provenance, 10.5, color, 2));
                    bottom.spawn(k.text(format!("{uncertainty} · {}", p.evidence), 10.5, SUBTLE, 0));
                });
            });
        }
    }
}

/// Saved studies, the running one, and the latest result's trade-off table.
pub(super) fn studies_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    body.spawn(k.caption("Run the same system several ways: compare alternatives for a part, or sweep one parameter. Studies are saved in the system file and rerun identically."));
    body.spawn(k.text("Start one from a part's inspector: Compare alternatives, or Sweep under Parameters.", size::DETAIL, FAINT, 0));
    if let Some((name, done, total)) = b.study_progress() {
        body.spawn(k.section("Running"));
        body.spawn(k.text(format!("{name}: {done} of {total} variants"), size::BODY, TEXT, 1));
        body.spawn((Node { border_radius: BorderRadius::all(Val::Px(3.)), height: Val::Px(6.), flex_shrink: 0., ..default() }, BackgroundColor(RAISED), children![(Node { border_radius: BorderRadius::all(Val::Px(3.)), width: Val::Percent(100. * done as f32 / total.max(1) as f32), ..default() }, BackgroundColor(ACCENT))]));
        body.spawn(wrap()).with_children(|r| {
            r.spawn(k.button("Cancel", BuildAction::CancelStudy, Look::Danger, true));
        });
    }
    tests_section(body, k, b);
    body.spawn(k.section(&format!("Saved  {}", b.document.studies.len())));
    if b.document.studies.is_empty() {
        body.spawn(k.caption("None yet."));
    }
    for (name, study) in &b.document.studies {
        let what = match &study.kind {
            sim_system::StudyKind::Compare { alternatives } => format!("compare {} with {} alternative{}", study.instance, alternatives.len(), if alternatives.len() == 1 { "" } else { "s" }),
            sim_system::StudyKind::Sweep { parameter, values } => format!("sweep {}.{parameter} over {} values", study.instance, values.len()),
        };
        body.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(6.), padding: UiRect::vertical(Val::Px(3.)), flex_shrink: 0., ..default() })
            .with_children(|row| {
                row.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1., min_width: Val::Px(0.), ..default() }, children![k.text(name, size::BODY, TEXT, 1), k.text(format!("{what} · {} s", num(study.duration)), size::DETAIL, SUBTLE, 0)]));
                row.spawn(k.button("Run", BuildAction::RunStudy(name.clone()), Look::Secondary, b.study.job.is_none()));
                row.spawn(k.button("×", BuildAction::RemoveStudy(name.clone()), Look::Ghost, true));
            });
    }
    if let Some(e) = &b.study.error {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, size::SMALL, DANGER, 0)]));
    }
    body.spawn(k.section(&format!("Runs  {}", b.runs.len())));
    if b.runs.is_empty() {
        body.spawn(k.note("Runs are kept automatically when you restart or edit structure, or with Save run."));
    }
    for (_, run) in b.runs.iter().take(20) {
        let picked = b.run_picks.contains(&run.id);
        body.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(6.), padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() })
            .with_children(|row| {
                row.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1., min_width: Val::Px(0.), ..default() }, children![
                    k.text(format!("{} · rev {} · {}", run.id, run.revision, run.fidelity), size::SMALL, TEXT, 1),
                    k.text(format!("{} s · seed {}{}", num(run.duration), run.seed, if run.note.is_empty() { String::new() } else { format!(" · {}", run.note) }), size::DETAIL, SUBTLE, 0)
                ]));
                row.spawn(k.chip(if picked { "✓" } else { "Pick" }, BuildAction::PickRun(run.id.clone()), picked, true));
                if b.replay.outcomes.get(&run.id).is_some_and(|o| o.status == "running") {
                    row.spawn(k.button("Cancel", BuildAction::CancelReplay, Look::Danger, true));
                } else {
                    row.spawn(k.button("Replay", BuildAction::ReplayRun(run.id.clone()), Look::Secondary, true));
                }
            });
        if let Some(o) = b.replay.outcomes.get(&run.id) {
            let color = match (o.status, o.max_rel_diff) {
                ("done", Some(d)) if d == 0. => OK,
                ("done", _) => WARN,
                ("error", _) => DANGER,
                _ => SUBTLE,
            };
            body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(color), k.text(o.headline(), size::SMALL, TEXT, 0)]));
            if o.status == "done" {
                body.spawn(k.text(format!("Headless rerun from t = 0 with the recorded document and config ({}, seed {}, {} s); a non-zero difference is a finding about this run.", o.fidelity, o.seed, num(o.duration)), size::DETAIL, FAINT, 0));
            }
            if o.edited_while_running {
                body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(WARN), k.text("The document was edited while this run recorded: the record keeps the final document, so the replay does not compare a clean run.", size::CAPTION, WARN, 0)]));
            }
        }
    }
    if b.run_picks.len() >= 2 {
        body.spawn(wrap()).with_children(|r| {
            r.spawn(k.button(&format!("Compare {} runs", b.run_picks.len()), BuildAction::CompareRuns, Look::Primary, true));
        });
    }
    let Some(r) = &b.study.result else { return };
    body.spawn(k.section(&format!("Result · {}", r.name)));
    for v in &r.variants {
        body.spawn((
            Node { border_radius: BorderRadius::all(Val::Px(6.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(2.), padding: UiRect::all(Val::Px(8.)), margin: UiRect::bottom(Val::Px(6.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor::all(BORDER),
            BackgroundColor(RAISED),
        ))
        .with_children(|card| {
            card.spawn(k.text(&v.label, size::BODY, TEXT, 2));
            if let Some(e) = &v.error {
                card.spawn(k.text(e, size::DETAIL, DANGER, 0));
            }
            for (m, x) in &v.metrics {
                k.property(card, m, &num(*x), "", None::<BuildAction>, false);
            }
            for d in v.derived.iter().filter(|d| ["efficiency", "self-locking", "ratio", "stall", "no-load"].iter().any(|w| d.name.contains(w))) {
                let value = if d.unit == "yes=1" { (if d.value >= 0.5 { "yes" } else { "no" }).to_string() } else { num(d.value) };
                k.property(card, &d.name, &value, if d.unit == "yes=1" || d.unit == "1" { "" } else { &d.unit }, None::<BuildAction>, false);
            }
            card.spawn(k.text(format!("{:.2} s wall time", v.wall_seconds), 10.5, FAINT, 0));
        });
    }
}

/// Acceptance tests: each with where its evidence stands and a Run button
/// (the same handler as REST `system_test {action: run}`).
fn tests_section(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    body.spawn(k.section(&format!("Tests  {}", b.document.tests.len())));
    if b.document.tests.is_empty() {
        body.spawn(k.text("No acceptance tests yet: save one with system_test {action: set} (requirements on observables).", size::DETAIL, FAINT, 0));
        return;
    }
    let standing = sim_runtime::system_evidence::standing(&b.document, b.path()).unwrap_or_default();
    let running = b.composition.test_run.as_ref().map(|r| r.name.clone());
    for (name, test) in &b.document.tests {
        let state = match standing.get(name) {
            Some(sim_runtime::system_evidence::Standing::NotAssessed) | None => "not assessed".to_string(),
            Some(sim_runtime::system_evidence::Standing::Current { verdict }) => format!("{verdict:?} (current)").to_lowercase(),
            Some(sim_runtime::system_evidence::Standing::Stale { verdict, changed }) => format!("{} (stale: {})", format!("{verdict:?}").to_lowercase(), changed.join("; ")),
        };
        body.spawn(k.text(format!("{name} · {} requirement{} · {} s", test.requirements.len(), if test.requirements.len() == 1 { "" } else { "s" }, test.duration_s), size::ITEM, TEXT, 1));
        body.spawn(k.text(state, size::DETAIL, FAINT, 0));
        body.spawn(wrap()).with_children(|r| {
            let busy = running.as_deref() == Some(name.as_str());
            r.spawn(k.button(if busy { "Running…" } else { "Run" }, BuildAction::RunTest(name.clone()), Look::Ghost, running.is_none()));
        });
    }
}
