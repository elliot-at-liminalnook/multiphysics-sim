//! Build-mode chrome: toolbar, library/outline/references sidebar, inspector
//! and status bar, drawn from one small set of themed components.
//!
//! Layout
//! ┌ toolbar: title · breadcrumb │ select/connect · group/ungroup/swap │ undo/redo │ run ┐
//! ├ sidebar (tabs) ┬──────── viewport ────────┬ inspector (selection) ┤
//! └ status: message                                   revision · parts · nets · state ┘
use super::*;
use bevy::input::mouse::MouseWheel;
use crate::ui_kit::{ACCENT, ACCENT_BG, BAR, BORDER, Corner, DANGER, Dock, FAINT, HOVER_BG, Kit, LEFT_WIDTH, Look, OK, RAISED, RIGHT_WIDTH, STATUSBAR, SUBTLE, SWITCHER_STRIP, TEXT, TOPBAR, Tint, UiFonts, WARN, WHEEL_LINE, divider, size, wheel_delta, wrap};

#[derive(Component)]
pub(super) enum Scroll {
    Left,
    Right,
}

/// Library categories, in palette order.
pub(super) const CATEGORIES: [&str; 12] = ["Subsystems", "Actuators", "Transmissions", "Mechanical", "Power", "Electrical", "Sensing", "Control", "Thermal", "Couplings", "Authored", "Other"];

pub(crate) fn category(domain: &str) -> &'static str {
    // Parts that name their palette section use it directly.
    if let Some(c) = CATEGORIES.iter().find(|c| **c == domain) {
        return c;
    }
    match domain {
        "system" | "library" => "Subsystems",
        "electrical" => "Electrical",
        "thermal" => "Thermal",
        "rotational" | "translational" | "multibody" => "Mechanical",
        "robot" | "actuator" => "Actuators",
        "control" => "Control",
        "sensing" => "Sensing",
        "bridge" | "magnetic" => "Couplings",
        "part" => "Authored",
        _ => "Other",
    }
}

/// The palette section of a library subsystem, from its declared interface:
/// a published gearmotor sits with the actuators, a screw with the
/// transmissions. Every subsystem also stays under "Subsystems".
pub(super) fn interface_category(interface: Option<&str>) -> &'static str {
    let i = interface.unwrap_or("");
    let has = |words: &[&str]| words.iter().any(|w| i.contains(w));
    if has(&["motor", "stepper", "servo", "actuator", "propulsion"]) {
        "Actuators"
    } else if has(&["gearbox", "screw", "belt", "gear"]) {
        "Transmissions"
    } else if has(&["battery", "regulator", "supply"]) {
        "Power"
    } else if has(&["bridge", "driver"]) {
        "Electrical"
    } else if has(&["sensor", "encoder"]) {
        "Sensing"
    } else {
        "Subsystems"
    }
}

pub(crate) fn tag_color(category: &str) -> Color {
    match category {
        // The accent's value (the source guard allows no literal equal to a token).
        "Subsystems" => ACCENT,
        "Electrical" => Color::srgb(0.87, 0.58, 0.33),
        "Thermal" => Color::srgb(0.92, 0.43, 0.38),
        "Mechanical" => Color::srgb(0.64, 0.69, 0.75),
        "Actuators" => Color::srgb(0.47, 0.76, 0.56),
        "Control" => Color::srgb(0.42, 0.60, 0.95),
        "Sensing" => Color::srgb(0.66, 0.55, 0.93),
        "Couplings" => Color::srgb(0.93, 0.76, 0.40),
        "Authored" => Color::srgb(0.95, 0.55, 0.80),
        "Transmissions" => Color::srgb(0.80, 0.70, 0.95),
        "Power" => Color::srgb(0.98, 0.85, 0.35),
        _ => Color::srgb(0.55, 0.60, 0.66),
    }
}

fn domain_of(kind: &InstanceKind) -> &str {
    match kind {
        InstanceKind::Element { component_type } => component_type.split('.').next().unwrap_or(""),
        InstanceKind::Subsystem { .. } => "system",
    }
}

/// Engineering-friendly number text: plain for everyday magnitudes, else
/// scientific, never long runs of zeros.
pub(crate) fn num(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return format!("{v}");
    }
    let a = v.abs();
    if (1e-3..1e6).contains(&a) {
        let s = format!("{:.6}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        // Keep about six significant digits.
        let digits: usize = s.chars().filter(|c| c.is_ascii_digit()).count();
        if digits > 7 { format!("{:.4}", v).trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
    } else {
        let s = format!("{:.3e}", v);
        let (m, e) = s.split_once('e').unwrap();
        format!("{}e{}", m.trim_end_matches('0').trim_end_matches('.'), e)
    }
}

fn kind_text(kind: &InstanceKind) -> String {
    match kind {
        InstanceKind::Element { component_type } => component_type.clone(),
        InstanceKind::Subsystem { definition } => format!("Subsystem · {definition}"),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rebuild_panel(mut commands: Commands, mut builder: ResMut<Builder>, panels: Query<Entity, With<BuilderPanel>>, scene: Res<SpatialScene>, fonts: Option<Res<UiFonts>>, buttons: Res<ButtonInput<MouseButton>>, scrolls:Query<(&ScrollPosition,&Scroll)>, selection: Res<Selection>, registry: Res<DocumentRegistry>, studies: Res<calibration::study::StudyOwner>, study_ui: Res<calibration::study::forms::StudyUi>) {
    let Some(fonts) = fonts else { return };
    if calibration::study::ui::presentation_changed(&studies,&study_ui) {
        builder.panel_dirty = true;
    }
    // Keep the pressed palette entity alive until its drag or click finishes.
    if buttons.pressed(MouseButton::Left) && builder.input.is_none() { return; }
    if !builder.panel_dirty {
        return;
    }
    let note_scroll=if builder.discussion.reset_scroll {0.}else{scrolls.iter().find(|(_,s)|matches!(s,Scroll::Left)).map(|(p,_)|p.y).unwrap_or(0.)};
    builder.discussion.reset_scroll=false;
    let side_scroll = builder.sidebar_scroll.take().unwrap_or(0.);
    builder.panel_dirty = false;
    for e in &panels {
        commands.entity(e).despawn();
    }
    let k = Kit::new(&fonts);
    let b = &*builder;
    // The shared selection's instance names at this level (`picked`).
    let selected = picked::names(&selection, &registry);
    toolbar(&mut commands, &k, b, &selected);
    sidebar(&mut commands, &k, b, note_scroll, side_scroll, &selected, &studies, &study_ui);
    inspector(&mut commands, &k, b, &scene, &selected);
    graph_dock(&mut commands, &k, b);
    let started = std::time::Instant::now();
    schematic::pane(&mut commands, &k, b, &selected);
    let schematic_ms = started.elapsed().as_secs_f64() * 1e3;
    status_bar(&mut commands, &k, b, &scene);
    if builder.schematic.visible {
        builder.schematic.build_ms = schematic_ms;
    }
}

fn toolbar(commands: &mut Commands, k: &Kit, b: &Builder, selected: &BTreeSet<String>) {
    let single = picked::only(selected);
    let subsystem = single.as_ref().is_some_and(|n| matches!(b.spec(n).map(|s| s.kind), Some(InstanceKind::Subsystem { .. })));
    let running = b.running();
    let (time, speed) = b.run.as_ref().and_then(|r| r.worker.shared().lock().ok().map(|s| (s.snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time).unwrap_or(0.), s.speed))).unwrap_or((0., 0.));
    commands
        .spawn((k.dock(Dock::Top { height: TOPBAR }, Node { padding: UiRect::axes(Val::Px(14.), Val::Px(0.)), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, ..default() }), BuilderPanel))
        .with_children(|bar| {
            // Left: product and where you are.
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.), ..default() }).with_children(|left| {
                if let Some(title) = &b.lesson {
                    left.spawn(k.button(&format!("‹ {title}"), BuildAction::Lessons, Look::Secondary, true));
                    left.spawn(divider());
                }
                left.spawn(k.text("System Builder", size::PRODUCT, TEXT, 2));
                left.spawn(divider());
                left.spawn(k.button(&b.document.title, BuildAction::Level(String::new()), Look::Ghost, true));
                let mut path = String::new();
                for part in sim_system::split_path(&b.level) {
                    path = sim_system::join_path(&path, part);
                    left.spawn(k.text("/", size::ITEM, FAINT, 0));
                    left.spawn(k.button(part, BuildAction::Level(path.clone()), if path == b.level { Look::Secondary } else { Look::Ghost }, true));
                }
            });
            // Center: tools.
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.), ..default() }).with_children(|mid| {
                mid.spawn(k.segments()).with_children(|seg| {
                    seg.spawn(k.segment("Select", BuildAction::SetMode(Mode::Select), b.mode == Mode::Select, true));
                    seg.spawn(k.segment("Annotate", BuildAction::SetMode(Mode::Annotate), b.mode == Mode::Annotate, b.input.is_none()));
                    seg.spawn(k.segment("Connect", BuildAction::SetMode(Mode::Connect), b.mode == Mode::Connect, true));
                });
                mid.spawn(divider());
                mid.spawn(k.button("Group", BuildAction::Group, Look::Secondary, !selected.is_empty()));
                mid.spawn(k.button("Ungroup", BuildAction::Ungroup, Look::Secondary, subsystem));
                mid.spawn(k.button("Swap", BuildAction::Swap, Look::Secondary, single.is_some()));
                mid.spawn(divider());
                mid.spawn(k.button("Undo", BuildAction::Undo, Look::Ghost, true));
                mid.spawn(k.button("Redo", BuildAction::Redo, Look::Ghost, true));
            });
            // Right: simulation.
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|right| {
                right.spawn(k.segments()).with_children(|seg| {
                    seg.spawn(k.segment("Detailed", BuildAction::ToggleRealtime, !b.realtime, b.realtime));
                    seg.spawn(k.segment("Realtime", BuildAction::ToggleRealtime, b.realtime, !b.realtime && b.document.realtime.is_some()));
                });
                right.spawn(k.segment("Schematic", BuildAction::ToggleSchematic, b.schematic.visible, true));
                right.spawn(k.segment("Graphs", BuildAction::ToggleGraphs, b.graphs.visible, true));
                right.spawn(divider());
                if b.run.is_some() {
                    right.spawn(k.caption(format!("t = {time:.3} s   {speed:.2}x real time")));
                    right.spawn(k.button("Save run", BuildAction::SaveRun, Look::Ghost, true));
                    right.spawn(k.button("Reset", BuildAction::Reset, Look::Ghost, true));
                    right.spawn(k.button("Step", BuildAction::Step, Look::Ghost, !running));
                }
                if running {
                    right.spawn(k.button("Pause", BuildAction::Pause, Look::Secondary, true));
                } else {
                    right.spawn(k.button(if b.run.is_some() { "Resume" } else { "Run" }, BuildAction::Run, Look::Primary, b.compile_error.is_none()));
                }
            });
        });
}

fn sidebar(commands: &mut Commands, k: &Kit, b: &Builder, note_scroll:f32, side_scroll: f32, selected: &BTreeSet<String>, studies: &calibration::study::StudyOwner, study_ui: &calibration::study::forms::StudyUi) {
    commands
        .spawn((k.dock(Dock::Left { top: TOPBAR, bottom: STATUSBAR, width: LEFT_WIDTH }, Node { flex_direction: FlexDirection::Column, ..default() }), BuilderPanel))
        .with_children(|side| {
            side.spawn(k.tab_strip()).with_children(|tabs| {
                for (label, tab) in [("Library", Tab::Library), ("Outline", Tab::Outline), ("Studies", Tab::Studies), ("References", Tab::References), ("Notes", Tab::Discussions), ("Systems", Tab::Systems), ("Actuators", Tab::Actuators), ("Gait lab", Tab::GaitLab)] {
                    if matches!(tab, Tab::Systems | Tab::Actuators | Tab::GaitLab) && b.open.shell.is_none() {
                        continue;
                    }
                    tabs.spawn(k.tab(label, BuildAction::Tab(tab), b.tab == tab));
                }
            });
            if b.tab==Tab::Discussions {
                side.spawn(Node{padding:UiRect::all(Val::Px(16.)),row_gap:Val::Px(10.),flex_direction:FlexDirection::Column,flex_shrink:0.,..default()}).with_children(|header|discussion_header(header,k,b));
                side.spawn((k.scroll_area(Node{padding:UiRect::axes(Val::Px(16.),Val::Px(8.)),row_gap:Val::Px(14.),flex_direction:FlexDirection::Column,flex_grow:1.,min_height:Val::Px(0.),..default()}, note_scroll),Scroll::Left)).with_children(|body|discussion_content(body,k,b,selected));
                if b.discussion.selected.is_some()||b.input.as_ref().is_some_and(|i|matches!(i.purpose,Purpose::Comment|Purpose::CommentAuthor|Purpose::ThreadTitle)) {
                    // A footer pinned under the scrolling messages (no kit widget for an in-flow footer).
                    side.spawn((Node{padding:UiRect::all(Val::Px(14.)),row_gap:Val::Px(8.),flex_direction:FlexDirection::Column,flex_shrink:0.,border:UiRect::top(Val::Px(1.)),..default()},BorderColor::all(BORDER),BackgroundColor(BAR))).with_children(|footer|discussion_composer(footer,k,b));
                }
                return;
            }
            side.spawn((
                k.scroll_area(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.), padding: UiRect::all(Val::Px(14.)), flex_grow: 1., ..default() }, side_scroll),
                Scroll::Left,
            ))
            .with_children(|body| match b.tab {
                Tab::Library => library_tab(body, k, b),
                Tab::Outline => outline_tab(body, k, b, selected),
                Tab::References => references_tab(body, k, b),
                Tab::Studies => studies_tab(body, k, b),
                Tab::Systems => systems_tab(body, k, b),
                Tab::Actuators => actuators_tab(body, k, b, studies, study_ui),
                Tab::GaitLab => gait_lab::tab(body, k, b),
                Tab::Discussions => {},
            });
        });
}

pub(crate) fn paragraph(col: &mut ChildSpawnerCommands, k: &Kit, title: &str, body: &str) {
    if body.is_empty() {
        return;
    }
    col.spawn(k.section(title));
    col.spawn(k.text(body, size::SMALL, Color::srgb(0.80, 0.83, 0.87), 0));
}

pub(crate) fn equations(col: &mut ChildSpawnerCommands, k: &Kit, lines: &[&str]) {
    if lines.is_empty() {
        return;
    }
    col.spawn(k.section("Equations"));
    col.spawn((
        Node { border_radius: BorderRadius::all(Val::Px(5.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.), padding: UiRect::all(Val::Px(9.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
        BorderColor::all(BORDER),
        BackgroundColor(BAR),
    ))
    .with_children(|box_| {
        for line in lines {
            box_.spawn(k.text(*line, size::SMALL, Color::srgb(0.86, 0.90, 0.80), 0));
        }
    });
}

/// Live traces of the plotted observables, under the viewport.
fn graph_dock(commands: &mut Commands, k: &Kit, b: &Builder) {
    if !b.graphs.visible {
        return;
    }
    // One `Dock::Under` above the status bar (the kit adds the switcher
    // strip, `SWITCHER_STRIP`), between the side columns.
    commands
        .spawn((k.dock(Dock::Under { left: LEFT_WIDTH, right: RIGHT_WIDTH, bottom: STATUSBAR, height: graphs::DOCK }, Node { padding: UiRect::all(Val::Px(10.)), column_gap: Val::Px(10.), ..default() }), BuilderPanel))
        .with_children(|dock| {
            if let Some(r) = &b.study.result {
                dock.spawn((Node { border_radius: BorderRadius::top(Val::Px(5.)), position_type: PositionType::Absolute, right: Val::Px(10.), top: Val::Px(-24.), column_gap: Val::Px(10.), padding: UiRect::axes(Val::Px(8.), Val::Px(3.)), align_items: AlignItems::Center, ..default() }, BackgroundColor(BAR)))
                    .with_children(|legend| {
                        legend.spawn(k.text(format!("Study {}", r.name), size::DETAIL, TEXT, 2));
                        for (label, color) in b.graphs.charts.iter().find(|c| !c.legend.is_empty()).map(|c| c.legend.clone()).unwrap_or_default() {
                            legend.spawn((Node { column_gap: Val::Px(5.), align_items: AlignItems::Center, ..default() }, children![k.dot(Color::srgb_u8(color[0], color[1], color[2])), k.text(label, size::DETAIL, SUBTLE, 0)]));
                        }
                        legend.spawn(k.button("Back to live", BuildAction::ClearStudy, Look::Ghost, true));
                    });
            }
            if b.graphs.charts.is_empty() {
                dock.spawn(k.caption(if b.run.is_none() { "Press Run (R) to record. Select a part to plot its speed, current or torque, or pin quantities from the inspector." } else { "Nothing plottable yet: select a part." }));
            }
            for (i, c) in b.graphs.charts.iter().enumerate() {
                let [r, g, bl] = c.color;
                let color = Color::srgb_u8(r, g, bl);
                dock.spawn((
                    Node { flex_direction: FlexDirection::Column, flex_grow: 1., flex_basis: Val::Px(0.), min_width: Val::Px(0.), row_gap: Val::Px(3.), ..default() },
                ))
                .with_children(|card| {
                    card.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(6.), flex_shrink: 0., ..default() }).with_children(|head| {
                        head.spawn((Node { column_gap: Val::Px(6.), align_items: AlignItems::Center, min_width: Val::Px(0.), overflow: Overflow::clip(), ..default() }, children![k.dot(color), k.text(&c.title, size::CAPTION, TEXT, 1)]));
                        let latest = c.latest.map(|v| format!("{} {}", num(v), c.unit)).unwrap_or_else(|| "–".into());
                        head.spawn(k.text(latest, size::CAPTION, color, 2));
                        if c.pinned {
                            head.spawn(k.button("×", BuildAction::Unpin(c.id.clone()), Look::Ghost, true));
                        }
                    });
                    if let Some(image) = b.graphs.images.get(i) {
                        card.spawn(k.chart_image(image.clone(), Node { flex_grow: 1., ..default() }, true)).with_children(|plot| {
                            let label = |v: f64| format!("{} {}", num(v), c.unit);
                            plot.spawn(k.chart_label(label(c.range.1), Corner::TopLeft));
                            plot.spawn(k.chart_label(label(c.range.0), Corner::BottomLeft));
                            let x = if c.x_label == "s" { format!("{:.2} – {:.2} s", c.window.0, c.window.1) } else { format!("{} {} – {}", c.x_label, num(c.window.0), num(c.window.1)) };
                            plot.spawn(k.chart_label(x, Corner::BottomRight));
                        });
                    }
                });
            }
        });
}

fn status_bar(commands: &mut Commands, k: &Kit, b: &Builder, scene: &SpatialScene) {
    let (state, color) = if b.open.pending().is_some() {
        ("Opening", ACCENT)
    } else if b.job.is_some() {
        ("Compiling", SUBTLE)
    } else if b.compile_error.as_deref().is_some_and(|e| e.contains("unconnected port")) {
        ("Unfinished wiring", WARN)
    } else if b.compile_error.is_some() {
        ("Does not compile", DANGER)
    } else if !b.findings.is_empty() {
        ("Incomplete", WARN)
    } else {
        ("Ready", OK)
    };
    commands
        .spawn((k.dock(Dock::Bottom { height: STATUSBAR }, Node { padding: UiRect::horizontal(Val::Px(14.)), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(16.), ..default() }), BuilderPanel))
        .with_children(|bar| {
            bar.spawn((Node { flex_shrink: 1., overflow: Overflow::clip(), ..default() }, children![k.caption(&b.status)]));
            bar.spawn(Node { column_gap: Val::Px(14.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }).with_children(|right| {
                right.spawn(k.note(format!("Revision {}", b.document.revision)));
                right.spawn(k.note(format!("{} parts", scene.spatial.parts.len())));
                right.spawn(k.note(format!("{} nets", scene.description.nets.len())));
                right.spawn((Node { column_gap: Val::Px(6.), align_items: AlignItems::Center, ..default() }, children![k.dot(color), k.text(state, size::CAPTION, SUBTLE, 1)]));
            });
        });
}

pub(super) fn scroll_panels(mut wheel: MessageReader<MouseWheel>, window: Single<&Window>, mut panels: Query<(&mut ScrollPosition, &Scroll)>) {
    let delta = wheel_delta(&mut wheel, WHEEL_LINE);
    let Some(p) = window.cursor_position() else { return };
    if p.y < TOPBAR || p.y > window.height() - STATUSBAR - SWITCHER_STRIP {
        return;
    }
    for (mut position, side) in &mut panels {
        let inside = match side {
            Scroll::Left => p.x < LEFT_WIDTH,
            Scroll::Right => p.x > window.width() - RIGHT_WIDTH,
        };
        if inside {
            position.y = (position.y - delta).max(0.0);
        }
    }
}

pub(crate) fn markdown_theme(k:&Kit)->crate::markdown::Theme{crate::markdown::Theme{regular:k.f.regular.clone(),strong:k.f.semibold.clone(),italic:k.f.italic.clone(),mono:k.f.mono.clone(),text:TEXT,muted:SUBTLE,accent:ACCENT,code:Color::srgb(0.91,0.75,0.49),surface:RAISED}}

mod inspector_panel;
mod notes_tab;
mod tabs;
use inspector_panel::inspector;
use notes_tab::{discussion_composer, discussion_content, discussion_header};
use tabs::{actuators_tab, library_tab, outline_tab, references_tab, studies_tab, systems_tab};
