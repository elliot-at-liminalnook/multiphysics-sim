//! The inspector dock: the level summary, the selected instance (identity,
//! parameters, ports, snapping, implementation), library cards, datasheets,
//! live readouts and the source preview.
use super::*;

pub(super) fn inspector(commands: &mut Commands, k: &Kit, b: &Builder, scene: &SpatialScene, selected: &BTreeSet<String>) {
    // The dock is itself the scroll area: its layout scrolls vertically (the
    // kit's `scroll_area` makes a node of its own, and a dock is one node).
    commands
        .spawn((
            k.dock(Dock::Right { top: TOPBAR, bottom: STATUSBAR, width: RIGHT_WIDTH }, Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.), padding: UiRect::all(Val::Px(16.)), overflow: Overflow::scroll_y(), ..default() }),
            ScrollPosition::default(),
            Scroll::Right,
            BuilderPanel, actions::RenderStamp::capture(b),
        ))
        .with_children(|col| {
            if b.reference.target.is_some(){source_preview(col,k,b);return;}
            let definition = b.definition();
            if let Some(item) = &b.preview {
                library_card(col, k, b, item, selected);
                return;
            }
            match (selected.len(), picked::only(selected)) {
                (0, _) => level_summary(col, k, b, definition.as_ref()),
                (1, Some(name)) => instance_inspector(col, k, b, &name, definition.as_ref()),
                (n, _) => {
                    k.header(col, &format!("{n} selected"), &selected.iter().cloned().collect::<Vec<_>>().join(", "));
                    col.spawn(Node { margin: UiRect::top(Val::Px(10.)), ..wrap() }).with_children(|r| {
                        r.spawn(k.button("Group into subsystem", BuildAction::Group, Look::Primary, true));
                        r.spawn(k.button("Delete", BuildAction::Delete, Look::Danger, true));
                    });
                    col.spawn(k.note("Nets that cross the selection become boundary ports of the new subsystem."));
                }
            }
            live_section(col, k, b, scene, selected);
        });
}

fn level_summary(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, definition: Option<&sim_system::Definition>) {
    let Some(d) = definition else { return };
    k.header(col, "Nothing selected", "Click a part in the viewport or the Outline. Shift-click to select several.");
    col.spawn(k.section("This level"));
    k.property(col, "Definition", &d.label, "", None::<BuildAction>, false);
    k.property(col, "Instances", &d.instances.len().to_string(), "", None::<BuildAction>, false);
    k.property(col, "Nets", &d.nets.len().to_string(), "", None::<BuildAction>, false);
    k.property(col, "Boundary ports", &d.ports.len().to_string(), "", None::<BuildAction>, false);
    if let Some(run) = &b.document.run {
        col.spawn(k.section("Run settings"));
        let integrator = match run.integrator {
            sim_system::IntegratorChoice::ImplicitMidpoint => "implicit midpoint",
            sim_system::IntegratorChoice::BackwardEuler => "backward Euler",
        };
        k.property(col, "Integrator", integrator, "", None::<BuildAction>, false);
        k.property(col, "Step", &format!("{:.0}", run.interval * 1e6), "us", None::<BuildAction>, false);
    }
    if let Some(p) = &b.document.realtime {
        col.spawn(k.section("Realtime profile"));
        k.property(col, "Step", &format!("{:.1}", p.interval * 1e3), "ms", None::<BuildAction>, false);
        for key in &p.observe {
            let bound = p.bounds.get(key).copied().unwrap_or(p.bound);
            let measured = p.measured.as_ref().and_then(|m| m.errors.get(key)).map(|e| format!("{:.2} % ", 100. * e)).unwrap_or_default();
            k.property(col, key, &format!("{measured}≤ {:.1} %", 100. * bound), "", None::<BuildAction>, false);
        }
        if let Some(m) = &p.measured {
            col.spawn(k.text(format!("Measured on {}: detailed {:.1}×, realtime {:.1}× realtime{}", m.host, m.detailed_speed, m.realtime_speed, if m.content_hash == sim_runtime::realtime_fidelity::measured_hash(&b.document) { "" } else { " — the model changed since; remeasure (sim-system realtime FILE --publish)" }), size::DETAIL, FAINT, 0));
        }
        if !p.notes.is_empty() {
            col.spawn(k.text(&p.notes, size::DETAIL, FAINT, 0));
        }
    }
    let updates = &b.updates;
    if !updates.is_empty() {
        col.spawn(k.section("Library updates"));
        for u in updates {
            col.spawn(k.text(format!("{}: {} → {}", u.id, u.imported_version.map(|v| format!("v{v}")).unwrap_or_else(|| "imported".into()), u.current_version.map(|v| format!("v{v}")).unwrap_or_else(|| "changed".into())), size::SMALL, WARN, 0));
        }
        col.spawn(wrap()).with_children(|r| {
            r.spawn(k.button("Update from library", BuildAction::SyncLibrary, Look::Primary, true));
        });
    }
    col.spawn(k.section(&format!("Review  {}", b.findings.len())));
    if let Some(e) = &b.compile_error {
        col.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, size::SMALL, DANGER, 0)]));
    }
    if b.findings.is_empty() && b.compile_error.is_none() {
        col.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(OK), k.text("Complete and compiles", size::BODY, SUBTLE, 0)]));
    }
    for f in b.findings.iter().take(24) {
        col.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() }, children![k.dot(WARN), k.caption(&f.message)]));
    }
}

fn instance_inspector(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, name: &str, definition: Option<&sim_system::Definition>) {
    let Some(spec) = b.spec(name) else { return };
    let cat = category(domain_of(&spec.kind));
    col.spawn((ImageNode::new(k.icon(&b.icon(&spec.kind))),Node{width:Val::Px(36.),height:Val::Px(36.),..default()}));
    col.spawn(k.title(if spec.label.is_empty() { name.to_string() } else { spec.label.clone() }));
    col.spawn((Node { column_gap: Val::Px(7.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(tag_color(cat)), k.text(format!("{cat}  ·  {}", kind_text(&spec.kind)), size::CAPTION, SUBTLE, 0)]));
    let rename_focus = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::Rename(name.to_string()));
    let shown_name = b.input.as_ref().filter(|_| rename_focus).map(|i| format!("{}|", i.buffer)).unwrap_or_else(|| name.to_string());
    col.spawn(k.section("Identity"));
    k.property(col, "Name", &shown_name, "", Some(BuildAction::Rename), rename_focus);
    k.property(col, "Path", &b.full_path(name), "", None::<BuildAction>, false);
    about_section(col, k, b, &spec);

    // Parameters.
    let declared: Vec<(String, String, Option<f64>)> = match &spec.kind {
        InstanceKind::Element { component_type } => b
            .registry
            .get(&component_type.as_str().into())
            .ok()
            .and_then(|d| d.parameters.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|p| !p.implementation_reference && !p.name.starts_with("initial.") && !p.name.contains('*') && !p.name.contains("jacobian") && !p.name.starts_with("dynamics.") && !p.name.ends_with(".events"))
            .map(|p| (p.name, p.unit, p.default))
            .collect(),
        InstanceKind::Subsystem { definition } => b.document.definitions.get(definition).map(|d| d.parameters.iter().map(|(k, p)| (k.clone(), p.unit.clone(), p.default)).collect()).unwrap_or_default(),
    };
    if !declared.is_empty() {
        col.spawn(k.section("Parameters"));
        let sweepable: Vec<(String, String, Option<f64>)> = declared.iter().filter(|(p, _, _)| !p.contains("correction") && !p.contains("smoothing")).cloned().collect();
        for (parameter, unit, default) in declared {
            let purpose = Purpose::Parameter { name: name.to_string(), parameter: parameter.clone() };
            let editing = b.input.as_ref().is_some_and(|i| i.purpose == purpose);
            let value = if editing {
                format!("{}|", b.input.as_ref().unwrap().buffer)
            } else {
                match spec.parameters.get(&parameter) {
                    Some(sim_system::ParameterBinding::Value { value, .. }) => num(*value),
                    Some(sim_system::ParameterBinding::Parameter { parameter }) => format!("= {parameter}"),
                    None => default.map(|d| format!("{} (default)", num(d))).unwrap_or_else(|| "required".into()),
                }
            };
            k.property(col, &parameter.replace('_', " "), &value, if unit == "1" { "" } else { &unit }, Some(BuildAction::Parameter(name.to_string(), parameter.clone())), editing);
            let sweeping = b.input.as_ref().is_some_and(|i| matches!(&i.purpose, Purpose::Sweep { name: n, parameter: p, .. } if n == name && *p == parameter));
            if sweeping {
                col.spawn(k.input(&b.input.as_ref().unwrap().buffer, "from to count, then Enter", BuildAction::SweepParameter(name.to_string(), parameter.clone()), true));
            }
        }
        if !b.level.is_empty() {
            col.spawn(wrap()).with_children(|r| {
                r.spawn(k.text("Expose to the level:", size::DETAIL, FAINT, 0));
                for (parameter, _, _) in &sweepable {
                    let exposed = matches!(spec.parameters.get(parameter), Some(sim_system::ParameterBinding::Parameter { .. }));
                    r.spawn(k.chip(&parameter.replace('_', " "), BuildAction::Expose(name.to_string(), parameter.clone()), exposed, !exposed));
                }
            });
        }
        col.spawn(wrap()).with_children(|r| {
            r.spawn(k.text("Sweep:", size::DETAIL, FAINT, 0));
            for (parameter, _, _) in &sweepable {
                r.spawn(k.chip(&parameter.replace('_', " "), BuildAction::SweepParameter(name.to_string(), parameter.clone()), false, true));
            }
        });
        col.spawn(k.text("Click a value to edit. Type $name to inherit a level parameter. Sweep runs one variant per value.", size::DETAIL, FAINT, 0));
    }

    col.spawn(k.section("Display position · metres"));
    col.spawn(k.button(&format!("{:.3}, {:.3}, {:.3}",spec.placement.position[0],spec.placement.position[1],spec.placement.position[2]), BuildAction::Position,Look::Secondary,true));
    if let Some(input)=b.input.as_ref().filter(|i|i.purpose==Purpose::Position){col.spawn(k.input(&input.buffer,"x y z in metres",BuildAction::Position,true));}
    col.spawn(k.text("Drag to arrange; X/Y/Z constrain, Alt bypasses snap. Display only.", size::DETAIL, FAINT, 0));
    // Ports.
    col.spawn(k.section("Ports"));
    if let Ok(ports) = Resolver::new(&b.document, &b.registry).instance_ports(&spec) {
        let connected: BTreeSet<Terminal> = definition.map(|d| d.nets.iter().flat_map(|n| n.terminals.clone()).collect()).unwrap_or_default();
        let top: Vec<_> = ports.iter().filter(|(p, _)| !ports.keys().any(|q| q != *p && p.starts_with(&format!("{q}.")))).collect();
        for (port, schema) in top {
            let t = Terminal::port(name, port);
            let is_connected = connected.contains(&t) || connected.iter().any(|c| matches!(c, Terminal::Port { instance, port: p } if instance == name && p.starts_with(&format!("{port}."))));
            let armed = b.connect_from.as_ref() == Some(&t);
            col.spawn((
                Button, crate::ui_kit::activation::Ordinary,
                BuildAction::Terminal(t.clone()),
                bevy::ui::prelude::AccessibleLabel::new(format!("Port {port}")),
                if armed { Tint::new(ACCENT_BG, HOVER_BG) } else { Tint::CLEAR },
                Node { border_radius: BorderRadius::all(Val::Px(4.)), column_gap: Val::Px(8.), align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(6.), Val::Px(4.)), flex_shrink: 0., ..default() },
                BackgroundColor(if armed { ACCENT_BG } else { Color::NONE }),
            ))
            .with_children(|row| {
                row.spawn(k.dot(if is_connected { OK } else { FAINT }));
                row.spawn((Node { flex_grow: 1., flex_direction: FlexDirection::Column, ..default() }, children![k.text(port, size::BODY, TEXT, 1), k.text(schema.as_ref().map(sim_system::commands::describe).unwrap_or_else(|| "untyped".into()), size::DETAIL, SUBTLE, 0)]));
                if is_connected && connected.contains(&t) {
                    row.spawn(k.button("Disconnect", BuildAction::Disconnect(t.clone()), Look::Ghost, true));
                }
            });
        }
        let hint = if b.connect_from.is_some() { "Now pick the other terminal (select another part if needed)." } else { "Click a port to start a connection." };
        col.spawn(k.text(hint, size::DETAIL, FAINT, 0));
        if let Some(d) = definition {
            if !d.ports.is_empty() && b.connect_from.is_some() {
                col.spawn(wrap()).with_children(|r| {
                    r.spawn(k.text("Level ports:", size::CAPTION, SUBTLE, 0));
                    for port in d.ports.keys() {
                        r.spawn(k.chip(port, BuildAction::Terminal(Terminal::boundary(port)), false, true));
                    }
                });
            }
        }
        if b.connect_from.is_some() {
            col.spawn(k.button("Cancel connection", BuildAction::CancelConnect, Look::Ghost, true));
        }
    }

    snap_section(col, k, b, name);

    // Implementation.
    col.spawn(k.section("Implementation"));
    k.property(col, "Current", &kind_text(&spec.kind), "", None::<BuildAction>, false);
    match &b.alternatives {
        Some((n, list)) if n == name => {
            if list.is_empty() {
                col.spawn(k.caption("No other implementation fits the connected ports."));
            }
            for (i, alt) in list.iter().enumerate().take(24) {
                let tag = match &alt.kind {
                    InstanceKind::Subsystem { .. } => "Subsystems",
                    InstanceKind::Element { component_type } => category(component_type.split('.').next().unwrap_or("")),
                };
                let subtitle = format!("{}{}", if alt.same_interface { "Same interface  ·  " } else { "" }, sim_system::commands::kind_label(&alt.kind));
                col.spawn(k.item(sim_core::icons::for_type(&subtitle), &alt.label, &subtitle, tag, BuildAction::SwapTo(i), false));
            }
        }
        _ => {
            col.spawn(wrap()).with_children(|r| {
                r.spawn(k.button("Show alternatives", BuildAction::Swap, Look::Secondary, true));
                r.spawn(k.button("Compare alternatives", BuildAction::CompareSelected, Look::Primary, true));
            });
            col.spawn(k.text("Compare runs this system once per same-interface alternative and overlays the results.", size::DETAIL, FAINT, 0));
        }
    }

    // Actions.
    col.spawn(k.section("Actions"));
    let subsystem = matches!(spec.kind, InstanceKind::Subsystem { .. });
    col.spawn(wrap()).with_children(|r| {
        if subsystem {
            r.spawn(k.button("Open", BuildAction::Open(name.to_string()), Look::Primary, true));
            r.spawn(k.button("Make unique", BuildAction::MakeUnique, Look::Secondary, true));
            r.spawn(k.button("Publish to library", BuildAction::SaveToLibrary, Look::Secondary, true));
        }
        r.spawn(k.button("Delete", BuildAction::Delete, Look::Danger, true));
    });
    if let InstanceKind::Subsystem { definition } = &spec.kind {
        let d = b.document.definitions.get(definition);
        let version = d.and_then(|d| d.version).map(|v| format!("v{v}")).unwrap_or_else(|| "unpublished".into());
        let source = d.and_then(|d| d.source.as_ref()).map(|s| format!(" · from {}", s.path)).unwrap_or_default();
        col.spawn(k.text(format!("{definition} · {version}{source}"), size::DETAIL, FAINT, 0));
        let here = Resolver::new(&b.document, &b.registry).placements(definition);
        let files = b.used_in.as_ref().filter(|(d, _)| d == definition).map(|(_, f)| f.clone()).unwrap_or_default();
        col.spawn(k.text(format!("Used {here}× in this file{}", if files.is_empty() { String::new() } else { format!("; in files: {}", files.iter().map(|(f, n)| format!("{} ({n})", std::path::Path::new(f).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default())).collect::<Vec<_>>().join(", ")) }), size::DETAIL, FAINT, 0));
    }
    col.spawn(k.text("Arrow keys move 5 mm (Shift: 1 mm). Page Up/Down lift.", size::DETAIL, FAINT, 0));
}

fn live_section(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, scene: &SpatialScene, selected: &BTreeSet<String>) {
    if let Some(name) = picked::only(selected).filter(|_| b.preview.is_none()) {
        let found = graphs::candidates(scene, &b.full_path(&name));
        if !found.is_empty() {
            col.spawn(k.section("Plot"));
            col.spawn(wrap()).with_children(|r| {
                for (id, title) in found.iter().take(16) {
                    let pinned = b.graphs.pinned.contains(id);
                    r.spawn(k.chip(title, if pinned { BuildAction::Unpin(id.clone()) } else { BuildAction::Pin(id.clone()) }, pinned, true));
                }
            });
            col.spawn(k.text("Pinned quantities stay in the graph dock (up to 4). With none pinned it follows the selection.", size::DETAIL, FAINT, 0));
        }
    }
    let Some(animation) = &scene.animation else { return };
    let Some(frame) = scene.frame() else { return };
    col.spawn(k.section("Live"));
    k.property(col, "Time", &format!("{:.4}", frame.time), "s", None::<BuildAction>, false);
    for r in animation.readouts.iter().take(12) {
        let unit = scene.description.observables.get(&r.observable).map(|o| sim_inspect::plot::unit(&scene.description, o)).unwrap_or("");
        let value = sim_inspect::animation::scalar(Some(frame), &r.observable).map(|v| format!("{:.2}", if unit == "K" { v.value - 273.15 } else { v.value })).unwrap_or_else(|| "–".into());
        k.property(col, &r.label, &value, if unit == "K" { "°C" } else { unit }, None::<BuildAction>, false);
    }
}

/// Values of an element's parameters as the inspector shows them.
fn explicit_values(spec: &InstanceSpec) -> BTreeMap<String, f64> {
    spec.parameters.iter().filter_map(|(k, v)| match v {
        sim_system::ParameterBinding::Value { value, .. } => Some((k.clone(), *value)),
        _ => None,
    }).collect()
}

fn derived_rows(col: &mut ChildSpawnerCommands, k: &Kit, derived: &[sim_core::DerivedValue]) {
    for d in derived {
        let value = if d.unit == "yes=1" { (if d.value >= 0.5 { "yes" } else { "no" }).to_string() } else { num(d.value) };
        let unit = if d.unit == "yes=1" || d.unit == "1" { "" } else { d.unit.as_str() };
        k.property(col, &d.name, &value, unit, None::<BuildAction>, false);
        col.spawn((Node { margin: UiRect { top: Val::Px(-3.), bottom: Val::Px(3.), ..default() }, flex_shrink: 0., ..default() }, children![k.text(&d.formula, 10.5, FAINT, 0)]));
    }
}

/// What the selected component is, its derived values at the current
/// parameters, and (expanded) how it works and what it trades off.
fn about_section(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, spec: &InstanceSpec) {
    match &spec.kind {
        InstanceKind::Element { component_type } => {
            let Some(notes) = b.registry.get(&component_type.as_str().into()).ok().and_then(|d| d.notes) else { return };
            col.spawn(k.section("About"));
            col.spawn(k.text(notes.summary, size::BODY, TEXT, 1));
            if notes.has_derived() {
                let values = library::effective_parameters(&b.registry, component_type, &explicit_values(spec));
                derived_rows(col, k, &notes.derive(&values));
            }
            col.spawn(wrap()).with_children(|r| {
                r.spawn(k.button(if b.show_notes { "Hide notes" } else { "How it works" }, BuildAction::ToggleNotes, Look::Ghost, true));
            });
            if b.show_notes {
                paragraph(col, k, "How it works", notes.explanation);
                equations(col, k, notes.equations);
                paragraph(col, k, "Trade-offs", notes.tradeoffs);
                paragraph(col, k, "Model limits", notes.limits);
            }
        }
        InstanceKind::Subsystem { definition } => {
            let Some(d) = b.document.definitions.get(definition) else { return };
            if !d.description.is_empty() {
                col.spawn(k.section("About"));
                col.spawn(k.text(&d.description, size::SMALL, Color::srgb(0.80, 0.83, 0.87), 0));
            }
        }
    }
}

/// For each port: what can snap onto it, recommended first.
fn snap_section(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, name: &str) {
    col.spawn(k.section("Snap on"));
    let Some(ports) = b.cached_suggestions(name) else {
        col.spawn(k.caption("Working out what fits…"));
        return;
    };
    col.spawn(k.text("Parts whose ports fit. Click one to add it next to this part and connect it in one undoable step.", size::DETAIL, FAINT, 0));
    for p in ports {
        let expanded = b.snap_expanded.contains(&p.port);
        col.spawn((
            Node { border_radius: BorderRadius::all(Val::Px(6.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(5.), padding: UiRect::all(Val::Px(8.)), margin: UiRect::top(Val::Px(6.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor::all(BORDER),
            BackgroundColor(RAISED),
        ))
        .with_children(|card| {
            card.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![
                k.dot(if p.connected_to.is_empty() { FAINT } else { OK }),
                k.text(&p.port, size::BODY, TEXT, 2),
                k.text(&p.schema, size::DETAIL, SUBTLE, 0),
            ]));
            if !p.connected_to.is_empty() {
                card.spawn(k.text(format!("on: {}", p.connected_to.join(", ")), size::DETAIL, FAINT, 0));
            }
            let shown: Vec<(usize, &sim_system::snap::Candidate)> = p.candidates.iter().enumerate().filter(|(_, c)| expanded || c.recommended).take(if expanded { 40 } else { 6 }).collect();
            card.spawn(wrap()).with_children(|r| {
                for (i, c) in &shown {
                    let look = if c.conflict.is_some() { Look::Ghost } else if c.recommended { Look::Chip(false) } else { Look::Secondary };
                    r.spawn(k.button(&c.label, BuildAction::Snap(p.port.clone(), *i), look, c.conflict.is_none()));
                }
            });
            if expanded {
                if let Some((_, c)) = shown.iter().find(|(_, c)| c.conflict.is_some()) {
                    card.spawn((Node { column_gap: Val::Px(6.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(WARN), k.text(c.conflict.as_deref().unwrap_or(""), size::DETAIL, SUBTLE, 0)]));
                }
            }
            let more = p.candidates.len().saturating_sub(shown.len());
            if more > 0 || expanded {
                card.spawn(wrap()).with_children(|r| {
                    r.spawn(k.button(&if expanded { "Fewer".to_string() } else { format!("{more} more") }, BuildAction::SnapMore(p.port.clone()), Look::Ghost, true));
                });
            }
        });
    }
}

/// A library item's card: everything to learn about it before placing it.
fn library_card(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, item: &PaletteItem, selected: &BTreeSet<String>) {
    let cat = category(&item.domain);
    col.spawn(k.text(&item.label, 17., TEXT, 2));
    col.spawn((Node { column_gap: Val::Px(7.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(tag_color(cat)), k.text(format!("{cat}  ·  {}", kind_text(&item.kind)), size::CAPTION, SUBTLE, 0)]));
    col.spawn(Node { margin: UiRect::vertical(Val::Px(8.)), ..wrap() }).with_children(|r| {
        r.spawn(k.button("Place on this level", BuildAction::PlacePreview, Look::Primary, true));
        r.spawn(k.button("Close", BuildAction::ClosePreview, Look::Ghost, true));
    });
    // Snap straight onto the selected part, where a port fits.
    if let Some(name) = picked::only(selected) {
        if let Some(ports) = b.cached_suggestions(&name) {
            let fits: Vec<(&str, &sim_system::snap::Candidate)> = ports.iter().filter_map(|p| p.candidates.iter().find(|c| c.kind == item.kind).map(|c| (p.port.as_str(), c))).collect();
            col.spawn(k.section(&format!("Attach to {name}")));
            if fits.is_empty() {
                col.spawn(k.caption(format!("No port of {name} fits this part.")));
            }
            for (port, c) in fits {
                col.spawn(wrap()).with_children(|r| {
                    r.spawn(k.button(&format!("{name}.{port}  ←  {}", c.port), BuildAction::AttachPreview(port.to_string()), if c.conflict.is_some() { Look::Ghost } else { Look::Secondary }, c.conflict.is_none()));
                });
                if let Some(conflict) = &c.conflict {
                    col.spawn((Node { column_gap: Val::Px(6.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(WARN), k.text(conflict, size::DETAIL, SUBTLE, 0)]));
                }
            }
        }
    }
    match &item.kind {
        InstanceKind::Element { component_type } => {
            let entry = b.element_entry(component_type);
            let notes = b.registry.get(&component_type.as_str().into()).ok().and_then(|d| d.notes);
            match notes {
                Some(n) => {
                    col.spawn(k.text(n.summary, size::ITEM, TEXT, 1));
                    paragraph(col, k, "How it works", n.explanation);
                    equations(col, k, n.equations);
                    paragraph(col, k, "Trade-offs", n.tradeoffs);
                    paragraph(col, k, "Model limits", n.limits);
                }
                None => {
                    col.spawn(k.caption("No notes yet for this component: ports and parameters below come straight from the registry."));
                }
            }
            if let Some(e) = entry {
                col.spawn(k.section("Ports"));
                for (port, schema) in &e.ports {
                    k.property(col, port, schema, "", None::<BuildAction>, false);
                }
                if !e.parameters.is_empty() {
                    col.spawn(k.section("Parameters"));
                    let typical: BTreeMap<&str, f64> = notes.map(|n| n.typical.iter().copied().collect()).unwrap_or_default();
                    for p in e.parameters.iter().filter(|p| !p.name.starts_with("initial.") && !p.name.contains('*')) {
                        let value = match (p.default, typical.get(p.name.as_str())) {
                            (_, Some(t)) => format!("{} (typical)", num(*t)),
                            (Some(d), None) => num(d),
                            (None, None) => "required".into(),
                        };
                        k.property(col, &p.name.replace('_', " "), &value, if p.unit == "1" { "" } else { &p.unit }, None::<BuildAction>, false);
                        if !p.help.is_empty() {
                            col.spawn((Node { margin: UiRect { top: Val::Px(-3.), bottom: Val::Px(3.), ..default() }, flex_shrink: 0., ..default() }, children![k.text(&p.help, 10.5, FAINT, 0)]));
                        }
                    }
                }
                if let Some(notes) = &e.notes {
                    if !notes.derived.is_empty() {
                        col.spawn(k.section("Derived at these values"));
                        derived_rows(col, k, &notes.derived);
                    }
                }
            }
            if let Some(sheet) = &b.preview_sheet {
                datasheet_section(col, k, sheet);
            } else {
                col.spawn(k.section("Datasheet"));
                col.spawn(k.note("No datasheet yet. Generate one: sim-system datasheet TYPE --write library/datasheets"));
            }
            if let Some(n) = notes.filter(|n| !n.pairs_with.is_empty()) {
                col.spawn(k.section("Pairs with"));
                col.spawn(wrap()).with_children(|r| {
                    for t in n.pairs_with {
                        let kind = InstanceKind::Element { component_type: t.to_string() };
                        let label = b.element_entry(t).map(|e| e.display_name.clone()).unwrap_or_else(|| t.to_string());
                        r.spawn(k.chip(&label, BuildAction::PreviewKind(kind), false, true));
                    }
                });
            }
        }
        InstanceKind::Subsystem { definition } => {
            let description = b.document.definitions.get(definition).map(|d| d.description.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| item.detail.clone());
            col.spawn(k.text(description, size::BODY, Color::srgb(0.80, 0.83, 0.87), 0));
        }
    }
}

/// A generated datasheet: checks, measured values and curve samples.
fn datasheet_section(col: &mut ChildSpawnerCommands, k: &Kit, sheet: &sim_runtime::bench::Datasheet) {
    col.spawn(k.section(&format!("Datasheet · {} bench", sheet.kind)));
    for c in &sheet.checks {
        col.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() }, children![k.dot(if c.passed { OK } else { DANGER }), k.text(format!("{}: {}", c.name, c.detail), size::CAPTION, SUBTLE, 0)]));
    }
    for (name, value) in &sheet.conditions {
        k.property(col, name, &num(*value), "", None::<BuildAction>, false);
    }
    for v in sheet.values.iter().filter(|v| v.name != "audit energy change") {
        let (value, unit) = match v.unit.as_str() {
            "yes=1" => ((if v.value >= 0.5 { "yes" } else { "no" }).to_string(), ""),
            "1" if v.name.contains("efficiency") => (format!("{:.1}", 100. * v.value), "%"),
            "1" => (num(v.value), ""),
            u => (num(v.value), u),
        };
        k.property(col, &v.name, &value, unit, None::<BuildAction>, false);
    }
    for c in sheet.curves.iter().take(3) {
        let sample: Vec<String> = c.points.iter().step_by((c.points.len() / 5).max(1)).map(|p| format!("{} → {}", num(p[0]), num(p[1]))).collect();
        col.spawn(k.text(format!("{} ({} vs {}): {}", c.name, c.y_label, c.x_label, sample.join(", ")), size::DETAIL, FAINT, 0));
    }
}

fn source_preview(col:&mut ChildSpawnerCommands,k:&Kit,b:&Builder){
    col.spawn(k.button("‹ Back to inspector",BuildAction::CloseReference,Look::Ghost,true));
    col.spawn(k.text("Source reference",17.,TEXT,2));
    match &b.reference.result{
        None=>{col.spawn(k.caption("Reading source…"));},
        Some(Err(e))=>{col.spawn(k.text(e, size::SMALL, WARN, 0));},
        Some(Ok(source))=>{
            col.spawn(k.text(format!("{}:{}",source.path,source.line), size::SMALL, ACCENT, 1));
            col.spawn(k.text("Read-only · current file", 10.5, FAINT, 0));
            for line in &source.lines{
                col.spawn((Node{width:Val::Percent(100.),min_width:Val::Px(0.),padding:UiRect::axes(Val::Px(5.),Val::Px(3.)),flex_shrink:0.,overflow:Overflow::clip(),..default()},BackgroundColor(if line.focused{RAISED}else{Color::NONE}))).with_children(|row|{
                    row.spawn((k.mono(format!("{:>3}  {}",line.number,line.text),11.,if line.focused{ACCENT}else{SUBTLE}),Node{width:Val::Percent(100.),min_width:Val::Px(0.),..default()}));
                });
            }
        }
    }
}
