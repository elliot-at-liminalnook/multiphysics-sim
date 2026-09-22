//! Build-mode chrome: toolbar, library/outline/references sidebar, inspector
//! and status bar, drawn from one small set of themed components.
//!
//! Layout
//! ┌ toolbar: title · breadcrumb │ select/connect · group/ungroup/swap │ undo/redo │ run ┐
//! ├ sidebar (tabs) ┬──────── viewport ────────┬ inspector (selection) ┤
//! └ status: message                                   revision · parts · nets · state ┘
use super::*;

pub const TOPBAR: f32 = 52.0;
pub const STATUSBAR: f32 = 30.0;
pub const LEFT_WIDTH: f32 = 300.0;
pub const RIGHT_WIDTH: f32 = 330.0;

// Theme: dark neutral surfaces, one accent, semantic warning/danger.
const BAR: Color = Color::srgb(0.071, 0.086, 0.106);
const SURFACE: Color = Color::srgb(0.094, 0.110, 0.137);
const RAISED: Color = Color::srgb(0.129, 0.149, 0.180);
const HOVER_BG: Color = Color::srgb(0.161, 0.184, 0.220);
const BORDER: Color = Color::srgb(0.180, 0.208, 0.247);
const TEXT: Color = Color::srgb(0.902, 0.922, 0.945);
const SUBTLE: Color = Color::srgb(0.600, 0.651, 0.710);
const FAINT: Color = Color::srgb(0.420, 0.463, 0.522);
const ACCENT_BG: Color = Color::srgb(0.098, 0.251, 0.239);
const ON_ACCENT: Color = Color::srgb(0.035, 0.090, 0.086);
const WARN: Color = Color::srgb(0.949, 0.749, 0.388);
const DANGER: Color = Color::srgb(0.937, 0.463, 0.435);
const OK: Color = Color::srgb(0.435, 0.816, 0.557);

#[derive(Resource, Clone)]
pub(super) struct UiFonts {
    regular: Handle<Font>,
    medium: Handle<Font>,
    semibold: Handle<Font>,
}

pub(super) fn load_fonts(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let load = |bytes: &[u8]| Font::try_from_bytes(bytes.to_vec()).expect("bundled IBM Plex Sans");
    commands.insert_resource(UiFonts {
        regular: fonts.add(load(include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf"))),
        medium: fonts.add(load(include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf"))),
        semibold: fonts.add(load(include_bytes!("../../assets/fonts/IBMPlexSans-SemiBold.ttf"))),
    });
}

#[derive(Component)]
pub(super) enum Scroll {
    Left,
    Right,
}

/// Background colors for idle and hovered states of a clickable element.
#[derive(Component, Clone, Copy)]
pub(super) struct Tint {
    idle: Color,
    hover: Color,
}

#[derive(Clone, Copy, PartialEq)]
enum Look {
    Primary,
    Secondary,
    Ghost,
    Danger,
    Tab(bool),
    Chip(bool),
    Segment(bool),
}

/// Library categories, in palette order.
pub(super) const CATEGORIES: [&str; 9] = ["Subsystems", "Electrical", "Thermal", "Mechanical", "Actuators", "Control", "Sensing", "Couplings", "Other"];

pub(super) fn category(domain: &str) -> &'static str {
    match domain {
        "system" | "library" => "Subsystems",
        "electrical" => "Electrical",
        "thermal" => "Thermal",
        "rotational" | "translational" | "multibody" => "Mechanical",
        "robot" | "actuator" => "Actuators",
        "control" => "Control",
        "sensing" => "Sensing",
        "bridge" | "magnetic" => "Couplings",
        _ => "Other",
    }
}

fn tag_color(category: &str) -> Color {
    match category {
        "Subsystems" => Color::srgb(0.30, 0.83, 0.75),
        "Electrical" => Color::srgb(0.87, 0.58, 0.33),
        "Thermal" => Color::srgb(0.92, 0.43, 0.38),
        "Mechanical" => Color::srgb(0.64, 0.69, 0.75),
        "Actuators" => Color::srgb(0.47, 0.76, 0.56),
        "Control" => Color::srgb(0.42, 0.60, 0.95),
        "Sensing" => Color::srgb(0.66, 0.55, 0.93),
        "Couplings" => Color::srgb(0.93, 0.76, 0.40),
        _ => Color::srgb(0.55, 0.60, 0.66),
    }
}

fn domain_of(kind: &InstanceKind) -> &str {
    match kind {
        InstanceKind::Element { component_type } => component_type.split('.').next().unwrap_or(""),
        InstanceKind::Subsystem { .. } => "system",
    }
}

struct Kit<'a> {
    f: &'a UiFonts,
}

impl Kit<'_> {
    fn text(&self, value: impl Into<String>, size: f32, color: Color, weight: u8) -> impl Bundle {
        let font = match weight {
            0 => self.f.regular.clone(),
            1 => self.f.medium.clone(),
            _ => self.f.semibold.clone(),
        };
        (Text::new(value), TextFont { font, font_size: size, ..default() }, TextColor(color), TextLayout::new_with_linebreak(bevy::text::LineBreak::WordBoundary))
    }

    fn button(&self, label: &str, action: BuildAction, look: Look, enabled: bool) -> impl Bundle {
        let (idle, hover, color, border, weight) = match look {
            Look::Primary => (ACCENT, Color::srgb(0.40, 0.90, 0.82), ON_ACCENT, ACCENT, 2),
            Look::Secondary => (RAISED, HOVER_BG, TEXT, BORDER, 1),
            Look::Ghost => (Color::NONE, HOVER_BG, SUBTLE, Color::NONE, 1),
            Look::Danger => (Color::NONE, Color::srgb(0.25, 0.12, 0.12), DANGER, Color::srgb(0.42, 0.20, 0.20), 1),
            Look::Tab(on) => (Color::NONE, if on { Color::NONE } else { HOVER_BG }, if on { TEXT } else { SUBTLE }, Color::NONE, if on { 2 } else { 1 }),
            Look::Chip(on) => (if on { ACCENT_BG } else { RAISED }, if on { ACCENT_BG } else { HOVER_BG }, if on { ACCENT } else { SUBTLE }, if on { ACCENT } else { BORDER }, 1),
            Look::Segment(on) => (if on { ACCENT_BG } else { Color::NONE }, if on { ACCENT_BG } else { HOVER_BG }, if on { ACCENT } else { SUBTLE }, Color::NONE, 1),
        };
        let (idle, hover, color) = if enabled { (idle, hover, color) } else { (idle, idle, FAINT) };
        let (pad_x, pad_y, size) = match look {
            Look::Chip(_) => (9., 3., 11.5),
            Look::Tab(_) => (4., 10., 13.),
            _ => (11., 5., 12.5),
        };
        let underline = matches!(look, Look::Tab(true));
        (
            Button,
            action,
            Tint { idle, hover },
            Node {
                padding: UiRect::axes(Val::Px(pad_x), Val::Px(pad_y)),
                border: if underline { UiRect::bottom(Val::Px(2.)) } else { UiRect::all(Val::Px(1.)) },
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_shrink: 0.,
                ..default()
            },
            BorderColor(if underline { ACCENT } else { border }),
            BorderRadius::all(Val::Px(if underline { 0. } else if matches!(look, Look::Chip(_)) { 10. } else { 5. })),
            BackgroundColor(idle),
            children![self.text(label, size, color, weight)],
        )
    }

    fn section(&self, title: &str) -> impl Bundle {
        (
            Node { margin: UiRect::top(Val::Px(14.)), padding: UiRect::bottom(Val::Px(6.)), border: UiRect::bottom(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor(BORDER),
            children![self.text(title.to_uppercase(), 10.5, FAINT, 2)],
        )
    }

    /// A clickable two-line list row with a domain tag.
    fn item(&self, title: &str, subtitle: &str, tag: &str, action: BuildAction, selected: bool) -> impl Bundle {
        let accent = tag_color(tag);
        (
            Button,
            action,
            Tint { idle: if selected { ACCENT_BG } else { Color::NONE }, hover: if selected { ACCENT_BG } else { HOVER_BG } },
            Node { padding: UiRect::axes(Val::Px(8.), Val::Px(6.)), column_gap: Val::Px(9.), align_items: AlignItems::Center, border: UiRect::left(Val::Px(2.)), flex_shrink: 0., ..default() },
            BorderColor(if selected { ACCENT } else { Color::NONE }),
            BorderRadius::all(Val::Px(4.)),
            BackgroundColor(if selected { ACCENT_BG } else { Color::NONE }),
            children![
                (Node { width: Val::Px(8.), height: Val::Px(8.), flex_shrink: 0., ..default() }, BorderRadius::all(Val::Px(2.)), BackgroundColor(accent)),
                (
                    Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(1.), flex_grow: 1., min_width: Val::Px(0.), ..default() },
                    children![self.text(title, 13., TEXT, 1), self.text(subtitle, 11., SUBTLE, 0)]
                )
            ],
        )
    }

    /// Key on the left, value on the right; the value is clickable when it
    /// has an action (for editing).
    fn property(&self, parent: &mut ChildSpawnerCommands, key: &str, value: &str, unit: &str, action: Option<BuildAction>, editing: bool) {
        let value_text = if unit.is_empty() { value.to_string() } else { format!("{value} {unit}") };
        parent
            .spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(10.), padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() })
            .with_children(|row| {
                row.spawn((self.text(key, 12.5, SUBTLE, 0), Node { flex_shrink: 1., ..default() }));
                let body = self.text(value_text, 12.5, if editing { TEXT } else { Color::srgb(0.84, 0.87, 0.91) }, 1);
                match action {
                    Some(action) => {
                        row.spawn((
                            Button,
                            action,
                            Tint { idle: if editing { RAISED } else { Color::NONE }, hover: HOVER_BG },
                            Node { padding: UiRect::axes(Val::Px(7.), Val::Px(3.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
                            BorderColor(if editing { ACCENT } else { BORDER }),
                            BorderRadius::all(Val::Px(4.)),
                            BackgroundColor(if editing { RAISED } else { Color::NONE }),
                            children![body],
                        ));
                    }
                    None => {
                        row.spawn((Node { padding: UiRect::axes(Val::Px(7.), Val::Px(3.)), flex_shrink: 0., ..default() }, children![body]));
                    }
                }
            });
    }

    fn input(&self, shown: &str, placeholder: &str, action: BuildAction, focused: bool) -> impl Bundle {
        let empty = shown.is_empty();
        (
            Button,
            action,
            Tint { idle: RAISED, hover: HOVER_BG },
            Node { padding: UiRect::axes(Val::Px(10.), Val::Px(7.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor(if focused { ACCENT } else { BORDER }),
            BorderRadius::all(Val::Px(5.)),
            BackgroundColor(RAISED),
            children![self.text(if empty && !focused { placeholder.to_string() } else if focused { format!("{shown}|") } else { shown.to_string() }, 12.5, if empty && !focused { FAINT } else { TEXT }, 0)],
        )
    }

    fn dot(&self, color: Color) -> impl Bundle {
        (Node { width: Val::Px(7.), height: Val::Px(7.), flex_shrink: 0., ..default() }, BorderRadius::all(Val::Px(4.)), BackgroundColor(color))
    }
}

/// Engineering-friendly number text: plain for everyday magnitudes, else
/// scientific, never long runs of zeros.
fn num(v: f64) -> String {
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

fn wrap() -> Node {
    Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(6.), row_gap: Val::Px(6.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }
}

fn divider() -> impl Bundle {
    (Node { width: Val::Px(1.), height: Val::Px(22.), margin: UiRect::horizontal(Val::Px(8.)), ..default() }, BackgroundColor(BORDER))
}

fn kind_text(kind: &InstanceKind) -> String {
    match kind {
        InstanceKind::Element { component_type } => component_type.clone(),
        InstanceKind::Subsystem { definition } => format!("Subsystem · {definition}"),
    }
}

pub(super) fn rebuild_panel(mut commands: Commands, mut builder: ResMut<Builder>, panels: Query<Entity, With<BuilderPanel>>, scene: Res<SpatialScene>, fonts: Option<Res<UiFonts>>) {
    let Some(fonts) = fonts else { return };
    if !builder.panel_dirty {
        return;
    }
    builder.panel_dirty = false;
    for e in &panels {
        commands.entity(e).despawn();
    }
    let k = Kit { f: &fonts };
    let b = &*builder;
    toolbar(&mut commands, &k, b);
    sidebar(&mut commands, &k, b);
    inspector(&mut commands, &k, b, &scene);
    status_bar(&mut commands, &k, b, &scene);
}

fn toolbar(commands: &mut Commands, k: &Kit, b: &Builder) {
    let single = b.only_selected();
    let subsystem = single.as_ref().is_some_and(|n| matches!(b.spec(n).map(|s| s.kind), Some(InstanceKind::Subsystem { .. })));
    let running = b.running();
    let (time, speed) = b.run.as_ref().and_then(|r| r.shared.lock().ok().map(|s| (s.snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time).unwrap_or(0.), s.speed))).unwrap_or((0., 0.));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.),
                right: Val::Px(0.),
                top: Val::Px(0.),
                height: Val::Px(TOPBAR),
                padding: UiRect::axes(Val::Px(14.), Val::Px(0.)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                border: UiRect::bottom(Val::Px(1.)),
                ..default()
            },
            BackgroundColor(BAR),
            BorderColor(BORDER),
            BuilderPanel,
        ))
        .with_children(|bar| {
            // Left: product and where you are.
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.), ..default() }).with_children(|left| {
                left.spawn(k.text("System Builder", 14., TEXT, 2));
                left.spawn(divider());
                left.spawn(k.button(&b.document.title, BuildAction::Level(String::new()), Look::Ghost, true));
                let mut path = String::new();
                for part in sim_system::split_path(&b.level) {
                    path = sim_system::join_path(&path, part);
                    left.spawn(k.text("/", 13., FAINT, 0));
                    left.spawn(k.button(part, BuildAction::Level(path.clone()), if path == b.level { Look::Secondary } else { Look::Ghost }, true));
                }
            });
            // Center: tools.
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.), ..default() }).with_children(|mid| {
                mid.spawn((Node { padding: UiRect::all(Val::Px(2.)), border: UiRect::all(Val::Px(1.)), column_gap: Val::Px(2.), ..default() }, BorderColor(BORDER), BorderRadius::all(Val::Px(6.))))
                    .with_children(|seg| {
                        seg.spawn(k.button("Select", BuildAction::SetMode(Mode::Select), Look::Segment(b.mode == Mode::Select), true));
                        seg.spawn(k.button("Connect", BuildAction::SetMode(Mode::Connect), Look::Segment(b.mode == Mode::Connect), true));
                    });
                mid.spawn(divider());
                mid.spawn(k.button("Group", BuildAction::Group, Look::Secondary, !b.selected.is_empty()));
                mid.spawn(k.button("Ungroup", BuildAction::Ungroup, Look::Secondary, subsystem));
                mid.spawn(k.button("Swap", BuildAction::Swap, Look::Secondary, single.is_some()));
                mid.spawn(divider());
                mid.spawn(k.button("Undo", BuildAction::Undo, Look::Ghost, true));
                mid.spawn(k.button("Redo", BuildAction::Redo, Look::Ghost, true));
            });
            // Right: simulation.
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|right| {
                if b.run.is_some() {
                    right.spawn(k.text(format!("t = {time:.3} s   {speed:.2}x real time"), 12., SUBTLE, 0));
                    right.spawn(k.button("Restart", BuildAction::Reset, Look::Ghost, true));
                }
                if running {
                    right.spawn(k.button("Pause", BuildAction::Pause, Look::Secondary, true));
                } else {
                    right.spawn(k.button(if b.run.is_some() { "Resume" } else { "Run" }, BuildAction::Run, Look::Primary, b.compile_error.is_none()));
                }
            });
        });
}

fn sidebar(commands: &mut Commands, k: &Kit, b: &Builder) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.),
                top: Val::Px(TOPBAR),
                bottom: Val::Px(STATUSBAR),
                width: Val::Px(LEFT_WIDTH),
                flex_direction: FlexDirection::Column,
                border: UiRect::right(Val::Px(1.)),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor(BORDER),
            BuilderPanel,
        ))
        .with_children(|side| {
            side.spawn((Node { padding: UiRect::horizontal(Val::Px(14.)), column_gap: Val::Px(18.), border: UiRect::bottom(Val::Px(1.)), flex_shrink: 0., ..default() }, BorderColor(BORDER)))
                .with_children(|tabs| {
                    for (label, tab) in [("Library", Tab::Library), ("Outline", Tab::Outline), ("References", Tab::References)] {
                        tabs.spawn(k.button(label, BuildAction::Tab(tab), Look::Tab(b.tab == tab), true));
                    }
                });
            side.spawn((
                Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.), padding: UiRect::all(Val::Px(14.)), overflow: Overflow::scroll_y(), flex_grow: 1., ..default() },
                ScrollPosition::default(),
                Scroll::Left,
            ))
            .with_children(|body| match b.tab {
                Tab::Library => library_tab(body, k, b),
                Tab::Outline => outline_tab(body, k, b),
                Tab::References => references_tab(body, k, b),
            });
        });
}

fn library_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    let focused = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::Filter);
    let shown = b.input.as_ref().filter(|_| focused).map(|i| i.buffer.clone()).unwrap_or_else(|| b.filter.clone());
    body.spawn(k.input(&shown, "Search components and subsystems   ( / )", BuildAction::Filter, focused));
    body.spawn(Node { margin: UiRect::vertical(Val::Px(8.)), flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(5.), row_gap: Val::Px(5.), flex_shrink: 0., ..default() })
        .with_children(|chips| {
            chips.spawn(k.button("All", BuildAction::Category(None), Look::Chip(b.category.is_none()), true));
            for c in CATEGORIES {
                if b.palette.iter().any(|p| category(&p.domain) == c) {
                    chips.spawn(k.button(c, BuildAction::Category(Some(c)), Look::Chip(b.category == Some(c)), true));
                }
            }
        });
    let items = b.filtered();
    body.spawn(k.text(format!("{} result{}  ·  click to place on this level", items.len(), if items.len() == 1 { "" } else { "s" }), 11., FAINT, 0));
    for (i, item) in items.iter().enumerate().take(PALETTE_ROWS) {
        let subtitle = match &item.kind {
            InstanceKind::Element { component_type } => component_type.clone(),
            InstanceKind::Subsystem { definition } => format!("{} · {definition}", if item.library_path.is_some() { "Library subsystem" } else { "Subsystem in this file" }),
        };
        body.spawn(k.item(&item.label, &subtitle, category(&item.domain), BuildAction::Place(i), false));
    }
    if items.len() > PALETTE_ROWS {
        body.spawn(k.text(format!("{} more. Refine the search.", items.len() - PALETTE_ROWS), 11.5, FAINT, 0));
    }
}

fn outline_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    let definition_id = b.definition_id().unwrap_or_else(|| b.document.root.clone());
    let Some(d) = b.document.definitions.get(&definition_id) else { return };
    let shared = Resolver::new(&b.document, &b.registry).placements(&definition_id);
    body.spawn(k.text(&d.label, 15., TEXT, 2));
    body.spawn(k.text(format!("{definition_id}{}", if shared > 1 { format!("  ·  shared by {shared} placements") } else { String::new() }), 11.5, SUBTLE, 0));
    if !b.level.is_empty() {
        body.spawn(Node { margin: UiRect::top(Val::Px(6.)), ..wrap() }).with_children(|r| {
            r.spawn(k.button("Up one level", BuildAction::Up, Look::Secondary, true));
        });
    }
    body.spawn(k.section(&format!("Contents  {}", d.instances.len())));
    if d.instances.is_empty() {
        body.spawn(k.text("Empty. Place components from the Library tab.", 12.5, SUBTLE, 0));
    }
    for (name, spec) in &d.instances {
        let title = if spec.label.is_empty() { name.clone() } else { format!("{}  ", spec.label) };
        let subtitle = format!("{name}  ·  {}", kind_text(&spec.kind));
        body.spawn(k.item(&title, &subtitle, category(domain_of(&spec.kind)), BuildAction::Select(name.clone()), b.selected.contains(name)));
    }
    if !d.ports.is_empty() {
        body.spawn(k.section("Boundary ports"));
        let resolver = Resolver::new(&b.document, &b.registry);
        for port in d.ports.keys() {
            let schema = resolver.boundary_schema(&definition_id, port, &mut Default::default()).ok().flatten();
            k.property(body, port, &schema.as_ref().map(sim_system::commands::describe).unwrap_or_else(|| "untyped".into()), "", None, false);
        }
    }
    if !d.nets.is_empty() {
        body.spawn(k.section(&format!("Nets  {}", d.nets.len())));
        for net in d.nets.iter().take(60) {
            let label = if net.label.is_empty() { "net".to_string() } else { net.label.clone() };
            body.spawn((
                Node { flex_direction: FlexDirection::Column, padding: UiRect::vertical(Val::Px(3.)), flex_shrink: 0., ..default() },
                children![k.text(label, 12., TEXT, 1), k.text(net.terminals.iter().map(|t| t.to_string()).collect::<Vec<_>>().join("  ·  "), 11., SUBTLE, 0)],
            ));
        }
    }
}

fn references_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    body.spawn(k.text("Reference images sit on a plane in this level. They are presentation only and never affect physics.", 12., SUBTLE, 0));
    let focused = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::ImportImage);
    let shown = b.input.as_ref().filter(|_| focused).map(|i| i.buffer.clone()).unwrap_or_default();
    body.spawn((Node { margin: UiRect::top(Val::Px(10.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.), flex_shrink: 0., ..default() }, children![k.input(&shown, "Paste an image path, then Enter", BuildAction::ImportImage, focused), k.text("Or drop a PNG or JPEG onto the viewport.", 11., FAINT, 0)]));
    let Some(d) = b.definition() else { return };
    let refs: Vec<_> = d.references.iter().filter(|(_, r)| r.view == ReferenceView::Spatial).collect();
    body.spawn(k.section(&format!("On this level  {}", refs.len())));
    for (id, r) in refs {
        let calibrating = b.calibrating.as_ref().is_some_and(|(c, _)| c == id);
        body.spawn((
            Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), padding: UiRect::all(Val::Px(10.)), margin: UiRect::bottom(Val::Px(6.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor(if calibrating { ACCENT } else { BORDER }),
            BorderRadius::all(Val::Px(6.)),
            BackgroundColor(RAISED),
        ))
        .with_children(|card| {
            card.spawn(k.text(&r.label, 13., TEXT, 1));
            let width_focus = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::ReferenceWidth(id.clone()));
            let width = b.input.as_ref().filter(|_| width_focus).map(|i| format!("{}|", i.buffer)).unwrap_or_else(|| format!("{:.3}", r.width));
            k.property(card, "Width", &width, "m", Some(BuildAction::Width(id.clone())), width_focus);
            k.property(card, "Opacity", &format!("{:.0}", r.opacity * 100.), "%", None, false);
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

fn inspector(commands: &mut Commands, k: &Kit, b: &Builder, scene: &SpatialScene) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.),
                top: Val::Px(TOPBAR),
                bottom: Val::Px(STATUSBAR),
                width: Val::Px(RIGHT_WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.),
                padding: UiRect::all(Val::Px(16.)),
                overflow: Overflow::scroll_y(),
                border: UiRect::left(Val::Px(1.)),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor(BORDER),
            ScrollPosition::default(),
            Scroll::Right,
            BuilderPanel,
        ))
        .with_children(|col| {
            let definition = b.definition();
            match (b.selected.len(), b.only_selected()) {
                (0, _) => level_summary(col, k, b, definition.as_ref()),
                (1, Some(name)) => instance_inspector(col, k, b, &name, definition.as_ref()),
                (n, _) => {
                    col.spawn(k.text(format!("{n} selected"), 16., TEXT, 2));
                    col.spawn(k.text(b.selected.iter().cloned().collect::<Vec<_>>().join(", "), 12., SUBTLE, 0));
                    col.spawn(Node { margin: UiRect::top(Val::Px(10.)), ..wrap() }).with_children(|r| {
                        r.spawn(k.button("Group into subsystem", BuildAction::Group, Look::Primary, true));
                        r.spawn(k.button("Delete", BuildAction::Delete, Look::Danger, true));
                    });
                    col.spawn(k.text("Nets that cross the selection become boundary ports of the new subsystem.", 11.5, FAINT, 0));
                }
            }
            live_section(col, k, scene);
        });
}

fn level_summary(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, definition: Option<&sim_system::Definition>) {
    let Some(d) = definition else { return };
    col.spawn(k.text("Nothing selected", 16., TEXT, 2));
    col.spawn(k.text("Click a part in the viewport or the Outline. Shift-click to select several.", 12., SUBTLE, 0));
    col.spawn(k.section("This level"));
    k.property(col, "Definition", &d.label, "", None, false);
    k.property(col, "Instances", &d.instances.len().to_string(), "", None, false);
    k.property(col, "Nets", &d.nets.len().to_string(), "", None, false);
    k.property(col, "Boundary ports", &d.ports.len().to_string(), "", None, false);
    if let Some(run) = &b.document.run {
        col.spawn(k.section("Run settings"));
        let integrator = match run.integrator {
            sim_system::IntegratorChoice::ImplicitMidpoint => "implicit midpoint",
            sim_system::IntegratorChoice::BackwardEuler => "backward Euler",
        };
        k.property(col, "Integrator", integrator, "", None, false);
        k.property(col, "Step", &format!("{:.0}", run.interval * 1e6), "us", None, false);
    }
    col.spawn(k.section(&format!("Review  {}", b.findings.len())));
    if let Some(e) = &b.compile_error {
        col.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, 12., DANGER, 0)]));
    }
    if b.findings.is_empty() && b.compile_error.is_none() {
        col.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(OK), k.text("Complete and compiles", 12.5, SUBTLE, 0)]));
    }
    for f in b.findings.iter().take(24) {
        col.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() }, children![k.dot(WARN), k.text(&f.message, 12., SUBTLE, 0)]));
    }
}

fn instance_inspector(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, name: &str, definition: Option<&sim_system::Definition>) {
    let Some(spec) = b.spec(name) else { return };
    let cat = category(domain_of(&spec.kind));
    col.spawn(k.text(if spec.label.is_empty() { name.to_string() } else { spec.label.clone() }, 16., TEXT, 2));
    col.spawn((Node { column_gap: Val::Px(7.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(tag_color(cat)), k.text(format!("{cat}  ·  {}", kind_text(&spec.kind)), 11.5, SUBTLE, 0)]));
    let rename_focus = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::Rename(name.to_string()));
    let shown_name = b.input.as_ref().filter(|_| rename_focus).map(|i| format!("{}|", i.buffer)).unwrap_or_else(|| name.to_string());
    col.spawn(k.section("Identity"));
    k.property(col, "Name", &shown_name, "", Some(BuildAction::Rename), rename_focus);
    k.property(col, "Path", &b.full_path(name), "", None, false);

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
            k.property(col, &parameter.replace('_', " "), &value, if unit == "1" { "" } else { &unit }, Some(BuildAction::Parameter(name.to_string(), parameter)), editing);
        }
        col.spawn(k.text("Click a value to edit. Type $name to inherit a level parameter.", 11., FAINT, 0));
    }

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
                Button,
                BuildAction::Terminal(t.clone()),
                Tint { idle: if armed { ACCENT_BG } else { Color::NONE }, hover: HOVER_BG },
                Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(6.), Val::Px(4.)), flex_shrink: 0., ..default() },
                BorderRadius::all(Val::Px(4.)),
                BackgroundColor(if armed { ACCENT_BG } else { Color::NONE }),
            ))
            .with_children(|row| {
                row.spawn(k.dot(if is_connected { OK } else { FAINT }));
                row.spawn((Node { flex_grow: 1., flex_direction: FlexDirection::Column, ..default() }, children![k.text(port, 12.5, TEXT, 1), k.text(schema.as_ref().map(sim_system::commands::describe).unwrap_or_else(|| "untyped".into()), 11., SUBTLE, 0)]));
                if is_connected && connected.contains(&t) {
                    row.spawn(k.button("Disconnect", BuildAction::Disconnect(t.clone()), Look::Ghost, true));
                }
            });
        }
        let hint = if b.connect_from.is_some() { "Now pick the other terminal (select another part if needed)." } else { "Click a port to start a connection." };
        col.spawn(k.text(hint, 11., FAINT, 0));
        if let Some(d) = definition {
            if !d.ports.is_empty() && b.connect_from.is_some() {
                col.spawn(wrap()).with_children(|r| {
                    r.spawn(k.text("Level ports:", 11.5, SUBTLE, 0));
                    for port in d.ports.keys() {
                        r.spawn(k.button(port, BuildAction::Terminal(Terminal::boundary(port)), Look::Chip(false), true));
                    }
                });
            }
        }
        if b.connect_from.is_some() {
            col.spawn(k.button("Cancel connection", BuildAction::CancelConnect, Look::Ghost, true));
        }
    }

    // Implementation.
    col.spawn(k.section("Implementation"));
    k.property(col, "Current", &kind_text(&spec.kind), "", None, false);
    match &b.alternatives {
        Some((n, list)) if n == name => {
            if list.is_empty() {
                col.spawn(k.text("No other implementation fits the connected ports.", 12., SUBTLE, 0));
            }
            for (i, alt) in list.iter().enumerate().take(24) {
                let tag = match &alt.kind {
                    InstanceKind::Subsystem { .. } => "Subsystems",
                    InstanceKind::Element { component_type } => category(component_type.split('.').next().unwrap_or("")),
                };
                let subtitle = format!("{}{}", if alt.same_interface { "Same interface  ·  " } else { "" }, sim_system::commands::kind_label(&alt.kind));
                col.spawn(k.item(&alt.label, &subtitle, tag, BuildAction::SwapTo(i), false));
            }
        }
        _ => {
            col.spawn(wrap()).with_children(|r| {
                r.spawn(k.button("Show alternatives", BuildAction::Swap, Look::Secondary, true));
            });
        }
    }

    // Actions.
    col.spawn(k.section("Actions"));
    let subsystem = matches!(spec.kind, InstanceKind::Subsystem { .. });
    col.spawn(wrap()).with_children(|r| {
        if subsystem {
            r.spawn(k.button("Open", BuildAction::Open(name.to_string()), Look::Primary, true));
            r.spawn(k.button("Make unique", BuildAction::MakeUnique, Look::Secondary, true));
            r.spawn(k.button("Save to library", BuildAction::SaveToLibrary, Look::Secondary, true));
        }
        r.spawn(k.button("Delete", BuildAction::Delete, Look::Danger, true));
    });
    col.spawn(k.text("Arrow keys move 5 mm (Shift: 1 mm). Page Up/Down lift.", 11., FAINT, 0));
}

fn live_section(col: &mut ChildSpawnerCommands, k: &Kit, scene: &SpatialScene) {
    let Some(animation) = &scene.animation else { return };
    let Some(frame) = scene.frame() else { return };
    col.spawn(k.section("Live"));
    k.property(col, "Time", &format!("{:.4}", frame.time), "s", None, false);
    for r in animation.readouts.iter().take(12) {
        let unit = scene.description.observables.get(&r.observable).map(|o| sim_inspect::plot::unit(&scene.description, o)).unwrap_or("");
        let value = sim_inspect::animation::scalar(Some(frame), &r.observable).map(|v| format!("{:.2}", if unit == "K" { v.value - 273.15 } else { v.value })).unwrap_or_else(|| "–".into());
        k.property(col, &r.label, &value, if unit == "K" { "°C" } else { unit }, None, false);
    }
}

fn status_bar(commands: &mut Commands, k: &Kit, b: &Builder, scene: &SpatialScene) {
    let (state, color) = if b.job.is_some() {
        ("Compiling", SUBTLE)
    } else if b.compile_error.is_some() {
        ("Does not compile", DANGER)
    } else if !b.findings.is_empty() {
        ("Incomplete", WARN)
    } else {
        ("Ready", OK)
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.),
                right: Val::Px(0.),
                bottom: Val::Px(0.),
                height: Val::Px(STATUSBAR),
                padding: UiRect::horizontal(Val::Px(14.)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                column_gap: Val::Px(16.),
                border: UiRect::top(Val::Px(1.)),
                ..default()
            },
            BackgroundColor(BAR),
            BorderColor(BORDER),
            BuilderPanel,
        ))
        .with_children(|bar| {
            bar.spawn((Node { flex_shrink: 1., overflow: Overflow::clip(), ..default() }, children![k.text(&b.status, 12., SUBTLE, 0)]));
            bar.spawn(Node { column_gap: Val::Px(14.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }).with_children(|right| {
                right.spawn(k.text(format!("Revision {}", b.document.revision), 11.5, FAINT, 0));
                right.spawn(k.text(format!("{} parts", scene.spatial.parts.len()), 11.5, FAINT, 0));
                right.spawn(k.text(format!("{} nets", scene.description.nets.len()), 11.5, FAINT, 0));
                right.spawn((Node { column_gap: Val::Px(6.), align_items: AlignItems::Center, ..default() }, children![k.dot(color), k.text(state, 11.5, SUBTLE, 1)]));
            });
        });
}

pub(super) fn scroll_panels(mut wheel: EventReader<MouseWheel>, window: Single<&Window>, mut panels: Query<(&mut ScrollPosition, &Scroll)>) {
    let delta = wheel.read().fold(0.0, |sum, e| {
        sum + match e.unit {
            MouseScrollUnit::Line => e.y * 28.0,
            MouseScrollUnit::Pixel => e.y,
        }
    });
    let Some(p) = window.cursor_position() else { return };
    if p.y < TOPBAR || p.y > window.height() - STATUSBAR {
        return;
    }
    for (mut position, side) in &mut panels {
        let inside = match side {
            Scroll::Left => p.x < LEFT_WIDTH,
            Scroll::Right => p.x > window.width() - RIGHT_WIDTH,
        };
        if inside {
            position.offset_y = (position.offset_y - delta).max(0.0);
        }
    }
}

pub(super) fn hover(mut buttons: Query<(&Interaction, &Tint, &mut BackgroundColor), Changed<Interaction>>) {
    for (interaction, tint, mut bg) in &mut buttons {
        bg.0 = match interaction {
            Interaction::Hovered | Interaction::Pressed => tint.hover,
            Interaction::None => tint.idle,
        };
    }
}
