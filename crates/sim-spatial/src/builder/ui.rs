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
pub(crate) const BAR: Color = Color::srgb(0.071, 0.086, 0.106);
pub(crate) const SURFACE: Color = Color::srgb(0.094, 0.110, 0.137);
pub(crate) const RAISED: Color = Color::srgb(0.129, 0.149, 0.180);
pub(crate) const HOVER_BG: Color = Color::srgb(0.161, 0.184, 0.220);
pub(crate) const BORDER: Color = Color::srgb(0.180, 0.208, 0.247);
pub(crate) const TEXT: Color = Color::srgb(0.902, 0.922, 0.945);
pub(crate) const SUBTLE: Color = Color::srgb(0.600, 0.651, 0.710);
pub(crate) const FAINT: Color = Color::srgb(0.420, 0.463, 0.522);
pub(crate) const ACCENT_BG: Color = Color::srgb(0.098, 0.251, 0.239);
pub(crate) const ON_ACCENT: Color = Color::srgb(0.035, 0.090, 0.086);
pub(crate) const WARN: Color = Color::srgb(0.949, 0.749, 0.388);
pub(crate) const DANGER: Color = Color::srgb(0.937, 0.463, 0.435);
pub(crate) const OK: Color = Color::srgb(0.435, 0.816, 0.557);

#[derive(Resource, Clone)]
pub(crate) struct UiFonts {
    pub(crate) regular: Handle<Font>,
    pub(crate) italic: Handle<Font>,
    pub(crate) mono: Handle<Font>,
    pub(crate) icons: BTreeMap<String, Handle<Image>>,
    pub(crate) medium: Handle<Font>,
    pub(crate) semibold: Handle<Font>,
}

impl UiFonts {
    /// The interface fonts and icons, added to the asset stores once while
    /// the app is built (`app::CorePlugin`): the first mode's OnEnter runs
    /// before Startup and spawns text with them. Shared by every mode.
    pub(crate) fn load(world: &mut World) -> Self {
        let icons = {
            let mut images = world.resource_mut::<Assets<Image>>();
            sim_core::icons::NAMES.iter().map(|name| {
                let image = Image::new(bevy::render::render_resource::Extent3d { width:48, height:48, depth_or_array_layers:1 }, bevy::render::render_resource::TextureDimension::D2, sim_core::icons::rgba(name,48), bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
                (name.to_string(), images.add(image))
            }).collect()
        };
        let mut fonts = world.resource_mut::<Assets<Font>>();
        let mut load = |bytes: &[u8]| fonts.add(Font::from_bytes(bytes.to_vec()));
        UiFonts { icons,
            italic: load(include_bytes!("../../assets/fonts/IBMPlexSans-Italic.ttf")),
            mono: load(include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf")),
            regular: load(include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf")),
            medium: load(include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf")),
            semibold: load(include_bytes!("../../assets/fonts/IBMPlexSans-SemiBold.ttf")),
        }
    }
}

#[derive(Component)]
pub(super) enum Scroll {
    Left,
    Right,
}

/// Background colors for idle and hovered states of a clickable element.
#[derive(Component, Clone, Copy)]
pub(crate) struct Tint {
    pub(crate) idle: Color,
    pub(crate) hover: Color,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Look {
    Primary,
    Secondary,
    Ghost,
    Danger,
    Tab(bool),
    Chip(bool),
    Segment(bool),
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
        "Subsystems" => Color::srgb(0.30, 0.83, 0.75),
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

pub(crate) struct Kit<'a> {
    pub(crate) f: &'a UiFonts,
}

impl Kit<'_> {
    pub(crate) fn text(&self, value: impl Into<String>, size: f32, color: Color, weight: u8) -> (Text, TextFont, TextColor, TextLayout) {
        let font = match weight {
            0 => self.f.regular.clone(),
            1 => self.f.medium.clone(),
            _ => self.f.semibold.clone(),
        };
        (Text::new(value), TextFont { font: font.into(), font_size: FontSize::Px(size), ..default() }, TextColor(color), TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter))
    }

    pub(crate) fn button<A: Component>(&self, label: &str, action: A, look: Look, enabled: bool) -> impl Bundle + use<A> {
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
            Look::Tab(_) => (3., 10., 11.5),
            _ => (11., 5., 12.5),
        };
        let underline = matches!(look, Look::Tab(true));
        (
            Button,
            action,
            ui_api::Enabled(enabled),
            Tint { idle, hover },
            Node { border_radius: BorderRadius::all(Val::Px(if underline { 0. } else if matches!(look, Look::Chip(_)) { 10. } else { 5. })),
                padding: UiRect::axes(Val::Px(pad_x), Val::Px(pad_y)),
                border: if underline { UiRect::bottom(Val::Px(2.)) } else { UiRect::all(Val::Px(1.)) },
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_shrink: 0.,
                ..default()
            },
            BorderColor::all(if underline { ACCENT } else { border }),
            BackgroundColor(idle),
            children![self.text(label, size, color, weight)],
        )
    }

    pub(crate) fn section(&self, title: &str) -> impl Bundle + use<> {
        (
            Node { margin: UiRect::top(Val::Px(14.)), padding: UiRect::bottom(Val::Px(6.)), border: UiRect::bottom(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor::all(BORDER),
            children![self.text(title.to_uppercase(), 10.5, FAINT, 2)],
        )
    }

    /// A clickable two-line list row with a domain tag.
    pub(crate) fn item<A: Component>(&self, icon: &str, title: &str, subtitle: &str, tag: &str, action: A, selected: bool) -> impl Bundle + use<A> {
        let accent = tag_color(tag);
        (
            Button,
            action,
            Tint { idle: if selected { ACCENT_BG } else { Color::NONE }, hover: if selected { ACCENT_BG } else { HOVER_BG } },
            Node { border_radius: BorderRadius::all(Val::Px(4.)), padding: UiRect::axes(Val::Px(8.), Val::Px(6.)), column_gap: Val::Px(9.), align_items: AlignItems::Center, border: UiRect::left(Val::Px(2.)), flex_shrink: 0., ..default() },
            BorderColor::all(if selected { ACCENT } else { Color::NONE }),
            BackgroundColor(if selected { ACCENT_BG } else { Color::NONE }),
            children![
                (Node { width: Val::Px(28.), height: Val::Px(28.), flex_shrink: 0., ..default() }, ImageNode::new(self.f.icons.get(icon).unwrap_or(&self.f.icons["component"]).clone()).with_color(accent)),
                (
                    Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(1.), flex_grow: 1., min_width: Val::Px(0.), ..default() },
                    children![self.text(title, 13., TEXT, 1), self.text(subtitle, 11., SUBTLE, 0)]
                )
            ],
        )
    }

    /// Key on the left, value on the right; the value is clickable when it
    /// has an action (for editing).
    pub(crate) fn property<A: Component>(&self, parent: &mut ChildSpawnerCommands, key: &str, value: &str, unit: &str, action: Option<A>, editing: bool) {
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
                            Node { border_radius: BorderRadius::all(Val::Px(4.)), padding: UiRect::axes(Val::Px(7.), Val::Px(3.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
                            BorderColor::all(if editing { ACCENT } else { BORDER }),
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

    pub(crate) fn input<A: Component>(&self, shown: &str, placeholder: &str, action: A, focused: bool) -> impl Bundle + use<A> {
        let empty = shown.is_empty();
        (
            Button,
            action,
            Tint { idle: RAISED, hover: HOVER_BG },
            Node { border_radius: BorderRadius::all(Val::Px(5.)), padding: UiRect::axes(Val::Px(10.), Val::Px(7.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor::all(if focused { ACCENT } else { BORDER }),
            BackgroundColor(RAISED),
            children![self.text(if empty && !focused { placeholder.to_string() } else if focused { format!("{shown}|") } else { shown.to_string() }, 12.5, if empty && !focused { FAINT } else { TEXT }, 0)],
        )
    }

    pub(crate) fn dot(&self, color: Color) -> impl Bundle + use<> {
        (Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Px(7.), height: Val::Px(7.), flex_shrink: 0., ..default() }, BackgroundColor(color))
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

pub(crate) fn wrap() -> Node {
    Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(6.), row_gap: Val::Px(6.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }
}

pub(crate) fn divider() -> impl Bundle + use<> {
    (Node { width: Val::Px(1.), height: Val::Px(22.), margin: UiRect::horizontal(Val::Px(8.)), ..default() }, BackgroundColor(BORDER))
}

fn kind_text(kind: &InstanceKind) -> String {
    match kind {
        InstanceKind::Element { component_type } => component_type.clone(),
        InstanceKind::Subsystem { definition } => format!("Subsystem · {definition}"),
    }
}

pub(super) fn rebuild_panel(mut commands: Commands, mut builder: ResMut<Builder>, panels: Query<Entity, With<BuilderPanel>>, scene: Res<SpatialScene>, fonts: Option<Res<UiFonts>>, buttons: Res<ButtonInput<MouseButton>>, scrolls:Query<(&ScrollPosition,&Scroll)>) {
    let Some(fonts) = fonts else { return };
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
    let k = Kit { f: &fonts };
    let b = &*builder;
    toolbar(&mut commands, &k, b);
    sidebar(&mut commands, &k, b, note_scroll, side_scroll);
    inspector(&mut commands, &k, b, &scene);
    graph_dock(&mut commands, &k, b);
    let started = std::time::Instant::now();
    schematic::pane(&mut commands, &k, b);
    let schematic_ms = started.elapsed().as_secs_f64() * 1e3;
    status_bar(&mut commands, &k, b, &scene);
    if builder.schematic.visible {
        builder.schematic.build_ms = schematic_ms;
    }
}

fn toolbar(commands: &mut Commands, k: &Kit, b: &Builder) {
    let single = b.only_selected();
    let subsystem = single.as_ref().is_some_and(|n| matches!(b.spec(n).map(|s| s.kind), Some(InstanceKind::Subsystem { .. })));
    let running = b.running();
    let (time, speed) = b.run.as_ref().and_then(|r| r.worker.shared().lock().ok().map(|s| (s.snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time).unwrap_or(0.), s.speed))).unwrap_or((0., 0.));
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
            BorderColor::all(BORDER),
            BuilderPanel,
        ))
        .with_children(|bar| {
            // Left: product and where you are.
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.), ..default() }).with_children(|left| {
                if let Some(title) = &b.lesson {
                    left.spawn(k.button(&format!("‹ {title}"), BuildAction::Lessons, Look::Secondary, true));
                    left.spawn(divider());
                }
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
                mid.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), padding: UiRect::all(Val::Px(2.)), border: UiRect::all(Val::Px(1.)), column_gap: Val::Px(2.), ..default() }, BorderColor::all(BORDER)))
                    .with_children(|seg| {
                        seg.spawn(k.button("Select", BuildAction::SetMode(Mode::Select), Look::Segment(b.mode == Mode::Select), true));
                        seg.spawn(k.button("Annotate", BuildAction::SetMode(Mode::Annotate), Look::Segment(b.mode == Mode::Annotate), b.input.is_none()));
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
                right.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), padding: UiRect::all(Val::Px(2.)), border: UiRect::all(Val::Px(1.)), column_gap: Val::Px(2.), ..default() }, BorderColor::all(BORDER)))
                    .with_children(|seg| {
                        seg.spawn(k.button("Detailed", BuildAction::ToggleRealtime, Look::Segment(!b.realtime), b.realtime));
                        seg.spawn(k.button("Realtime", BuildAction::ToggleRealtime, Look::Segment(b.realtime), !b.realtime && b.document.realtime.is_some()));
                    });
                right.spawn(k.button("Schematic", BuildAction::ToggleSchematic, Look::Segment(b.schematic.visible), true));
                right.spawn(k.button("Graphs", BuildAction::ToggleGraphs, Look::Segment(b.graphs.visible), true));
                right.spawn(divider());
                if b.run.is_some() {
                    right.spawn(k.text(format!("t = {time:.3} s   {speed:.2}x real time"), 12., SUBTLE, 0));
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

fn sidebar(commands: &mut Commands, k: &Kit, b: &Builder, note_scroll:f32, side_scroll: f32) {
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
            BorderColor::all(BORDER),
            BuilderPanel,
        ))
        .with_children(|side| {
            side.spawn((Node { padding: UiRect::horizontal(Val::Px(10.)), column_gap: Val::Px(6.), flex_wrap: FlexWrap::Wrap, border: UiRect::bottom(Val::Px(1.)), flex_shrink: 0., ..default() }, BorderColor::all(BORDER)))
                .with_children(|tabs| {
                    for (label, tab) in [("Library", Tab::Library), ("Outline", Tab::Outline), ("Studies", Tab::Studies), ("References", Tab::References), ("Notes", Tab::Discussions), ("Systems", Tab::Systems), ("Actuators", Tab::Actuators), ("Gait lab", Tab::GaitLab)] {
                        if matches!(tab, Tab::Systems | Tab::Actuators | Tab::GaitLab) && b.open.shell.is_none() {
                            continue;
                        }
                        tabs.spawn(k.button(label, BuildAction::Tab(tab), Look::Tab(b.tab == tab), true));
                    }
                });
            if b.tab==Tab::Discussions {
                side.spawn(Node{padding:UiRect::all(Val::Px(16.)),row_gap:Val::Px(10.),flex_direction:FlexDirection::Column,flex_shrink:0.,..default()}).with_children(|header|discussion_header(header,k,b));
                side.spawn((Node{padding:UiRect::axes(Val::Px(16.),Val::Px(8.)),row_gap:Val::Px(14.),flex_direction:FlexDirection::Column,overflow:Overflow::scroll_y(),flex_grow:1.,min_height:Val::Px(0.),..default()},ScrollPosition(Vec2::new(0.0, note_scroll)),Scroll::Left)).with_children(|body|discussion_content(body,k,b));
                if b.discussion.selected.is_some()||b.input.as_ref().is_some_and(|i|matches!(i.purpose,Purpose::Comment|Purpose::CommentAuthor|Purpose::ThreadTitle)) {
                    side.spawn((Node{padding:UiRect::all(Val::Px(14.)),row_gap:Val::Px(8.),flex_direction:FlexDirection::Column,flex_shrink:0.,border:UiRect::top(Val::Px(1.)),..default()},BorderColor::all(BORDER),BackgroundColor(BAR))).with_children(|footer|discussion_composer(footer,k,b));
                }
                return;
            }
            side.spawn((
                Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.), padding: UiRect::all(Val::Px(14.)), overflow: Overflow::scroll_y(), flex_grow: 1., ..default() },
                ScrollPosition(Vec2::new(0.0, side_scroll)),
                Scroll::Left,
            ))
            .with_children(|body| match b.tab {
                Tab::Library => library_tab(body, k, b),
                Tab::Outline => outline_tab(body, k, b),
                Tab::References => references_tab(body, k, b),
                Tab::Studies => studies_tab(body, k, b),
                Tab::Systems => systems_tab(body, k, b),
                Tab::Actuators => actuators_tab(body, k, b),
                Tab::GaitLab => gait_lab::tab(body, k, b),
                Tab::Discussions => {},
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
                if b.palette.iter().any(|p| category(&p.domain) == c || (c == "Subsystems" && matches!(p.kind, InstanceKind::Subsystem { .. }))) {
                    chips.spawn(k.button(c, BuildAction::Category(Some(c)), Look::Chip(b.category == Some(c)), true));
                }
            }
        });
    let grid=b.grid();
    body.spawn(wrap()).with_children(|r| {
        r.spawn(k.button("Grid",BuildAction::GridVisible,Look::Chip(grid.visible),true));
        r.spawn(k.button("Snap",BuildAction::GridSnap,Look::Chip(grid.snap),true));
        r.spawn(k.button(&format!("{:?}",grid.plane),BuildAction::GridPlane,Look::Secondary,true));
        r.spawn(k.button(&format!("{} mm",grid.spacing_m*1000.),BuildAction::GridSpacing,Look::Secondary,true));
        r.spawn(k.button("Origin",BuildAction::GridOrigin,Look::Secondary,true));
    });
    for purpose in [Purpose::GridSpacing,Purpose::GridOrigin] {if let Some(i)=b.input.as_ref().filter(|i|i.purpose==purpose){body.spawn(k.input(&i.buffer,"Metres · Enter to save",BuildAction::GridSpacing,true));}}
    body.spawn(k.text("Display layout only · drag a component into the grid",11.,FAINT,0));
    let items = b.filtered();
    body.spawn(k.text(format!("{} result{}  ·  click for details, then place or snap", items.len(), if items.len() == 1 { "" } else { "s" }), 11., FAINT, 0));
    for (i, item) in items.iter().enumerate().take(PALETTE_ROWS) {
        let subtitle = match &item.kind {
            InstanceKind::Element { component_type } => component_type.clone(),
            InstanceKind::Subsystem { definition } => format!("{} · {definition}", if item.library_path.is_some() { "Library subsystem" } else { "Subsystem in this file" }),
        };
        let shown = b.preview.as_ref().is_some_and(|p| p.kind == item.kind);
        body.spawn(k.item(&b.icon(&item.kind), &item.label, &subtitle, category(&item.domain), BuildAction::Preview(i), shown)).observe(placement::start_palette);
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
        body.spawn(k.item(&b.icon(&spec.kind), &title, &subtitle, category(domain_of(&spec.kind)), BuildAction::Select(name.clone()), b.selected.contains(name)));
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
            Node { border_radius: BorderRadius::all(Val::Px(6.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), padding: UiRect::all(Val::Px(10.)), margin: UiRect::bottom(Val::Px(6.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
            BorderColor::all(if calibrating { ACCENT } else { BORDER }),
            BackgroundColor(RAISED),
        ))
        .with_children(|card| {
            card.spawn(k.text(&r.label, 13., TEXT, 1));
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
            BorderColor::all(BORDER),
            ScrollPosition::default(),
            Scroll::Right,
            BuilderPanel,
        ))
        .with_children(|col| {
            if b.reference.target.is_some(){source_preview(col,k,b);return;}
            let definition = b.definition();
            if let Some(item) = &b.preview {
                library_card(col, k, b, item);
                return;
            }
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
            live_section(col, k, b, scene);
        });
}

fn level_summary(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, definition: Option<&sim_system::Definition>) {
    let Some(d) = definition else { return };
    col.spawn(k.text("Nothing selected", 16., TEXT, 2));
    col.spawn(k.text("Click a part in the viewport or the Outline. Shift-click to select several.", 12., SUBTLE, 0));
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
            col.spawn(k.text(format!("Measured on {}: detailed {:.1}×, realtime {:.1}× realtime{}", m.host, m.detailed_speed, m.realtime_speed, if m.content_hash == sim_runtime::realtime_fidelity::measured_hash(&b.document) { "" } else { " — the model changed since; remeasure (sim-system realtime FILE --publish)" }), 11., FAINT, 0));
        }
        if !p.notes.is_empty() {
            col.spawn(k.text(&p.notes, 11., FAINT, 0));
        }
    }
    let updates = &b.updates;
    if !updates.is_empty() {
        col.spawn(k.section("Library updates"));
        for u in updates {
            col.spawn(k.text(format!("{}: {} → {}", u.id, u.imported_version.map(|v| format!("v{v}")).unwrap_or_else(|| "imported".into()), u.current_version.map(|v| format!("v{v}")).unwrap_or_else(|| "changed".into())), 12., WARN, 0));
        }
        col.spawn(wrap()).with_children(|r| {
            r.spawn(k.button("Update from library", BuildAction::SyncLibrary, Look::Primary, true));
        });
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
    col.spawn((ImageNode::new(k.f.icons.get(&b.icon(&spec.kind)).unwrap_or(&k.f.icons["component"]).clone()),Node{width:Val::Px(36.),height:Val::Px(36.),..default()}));
    col.spawn(k.text(if spec.label.is_empty() { name.to_string() } else { spec.label.clone() }, 16., TEXT, 2));
    col.spawn((Node { column_gap: Val::Px(7.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(tag_color(cat)), k.text(format!("{cat}  ·  {}", kind_text(&spec.kind)), 11.5, SUBTLE, 0)]));
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
                r.spawn(k.text("Expose to the level:", 11., FAINT, 0));
                for (parameter, _, _) in &sweepable {
                    let exposed = matches!(spec.parameters.get(parameter), Some(sim_system::ParameterBinding::Parameter { .. }));
                    r.spawn(k.button(&parameter.replace('_', " "), BuildAction::Expose(name.to_string(), parameter.clone()), Look::Chip(exposed), !exposed));
                }
            });
        }
        col.spawn(wrap()).with_children(|r| {
            r.spawn(k.text("Sweep:", 11., FAINT, 0));
            for (parameter, _, _) in &sweepable {
                r.spawn(k.button(&parameter.replace('_', " "), BuildAction::SweepParameter(name.to_string(), parameter.clone()), Look::Chip(false), true));
            }
        });
        col.spawn(k.text("Click a value to edit. Type $name to inherit a level parameter. Sweep runs one variant per value.", 11., FAINT, 0));
    }

    col.spawn(k.section("Display position · metres"));
    col.spawn(k.button(&format!("{:.3}, {:.3}, {:.3}",spec.placement.position[0],spec.placement.position[1],spec.placement.position[2]), BuildAction::Position,Look::Secondary,true));
    if let Some(input)=b.input.as_ref().filter(|i|i.purpose==Purpose::Position){col.spawn(k.input(&input.buffer,"x y z in metres",BuildAction::Position,true));}
    col.spawn(k.text("Drag to arrange; X/Y/Z constrain, Alt bypasses snap. Display only.",11.,FAINT,0));
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
                Node { border_radius: BorderRadius::all(Val::Px(4.)), column_gap: Val::Px(8.), align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(6.), Val::Px(4.)), flex_shrink: 0., ..default() },
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

    snap_section(col, k, b, name);

    // Implementation.
    col.spawn(k.section("Implementation"));
    k.property(col, "Current", &kind_text(&spec.kind), "", None::<BuildAction>, false);
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
                col.spawn(k.item(sim_core::icons::for_type(&subtitle), &alt.label, &subtitle, tag, BuildAction::SwapTo(i), false));
            }
        }
        _ => {
            col.spawn(wrap()).with_children(|r| {
                r.spawn(k.button("Show alternatives", BuildAction::Swap, Look::Secondary, true));
                r.spawn(k.button("Compare alternatives", BuildAction::CompareSelected, Look::Primary, true));
            });
            col.spawn(k.text("Compare runs this system once per same-interface alternative and overlays the results.", 11., FAINT, 0));
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
        col.spawn(k.text(format!("{definition} · {version}{source}"), 11., FAINT, 0));
        let here = Resolver::new(&b.document, &b.registry).placements(definition);
        let files = b.used_in.as_ref().filter(|(d, _)| d == definition).map(|(_, f)| f.clone()).unwrap_or_default();
        col.spawn(k.text(format!("Used {here}× in this file{}", if files.is_empty() { String::new() } else { format!("; in files: {}", files.iter().map(|(f, n)| format!("{} ({n})", std::path::Path::new(f).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default())).collect::<Vec<_>>().join(", ")) }), 11., FAINT, 0));
    }
    col.spawn(k.text("Arrow keys move 5 mm (Shift: 1 mm). Page Up/Down lift.", 11., FAINT, 0));
}

fn live_section(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, scene: &SpatialScene) {
    if let Some(name) = b.only_selected().filter(|_| b.preview.is_none()) {
        let found = graphs::candidates(scene, &b.full_path(&name));
        if !found.is_empty() {
            col.spawn(k.section("Plot"));
            col.spawn(wrap()).with_children(|r| {
                for (id, title) in found.iter().take(16) {
                    let pinned = b.graphs.pinned.contains(id);
                    r.spawn(k.button(title, if pinned { BuildAction::Unpin(id.clone()) } else { BuildAction::Pin(id.clone()) }, Look::Chip(pinned), true));
                }
            });
            col.spawn(k.text("Pinned quantities stay in the graph dock (up to 4). With none pinned it follows the selection.", 11., FAINT, 0));
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

pub(crate) fn paragraph(col: &mut ChildSpawnerCommands, k: &Kit, title: &str, body: &str) {
    if body.is_empty() {
        return;
    }
    col.spawn(k.section(title));
    col.spawn(k.text(body, 12., Color::srgb(0.80, 0.83, 0.87), 0));
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
            box_.spawn(k.text(*line, 12., Color::srgb(0.86, 0.90, 0.80), 0));
        }
    });
}

/// What the selected component is, its derived values at the current
/// parameters, and (expanded) how it works and what it trades off.
fn about_section(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, spec: &InstanceSpec) {
    match &spec.kind {
        InstanceKind::Element { component_type } => {
            let Some(notes) = b.registry.get(&component_type.as_str().into()).ok().and_then(|d| d.notes) else { return };
            col.spawn(k.section("About"));
            col.spawn(k.text(notes.summary, 12.5, TEXT, 1));
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
                col.spawn(k.text(&d.description, 12., Color::srgb(0.80, 0.83, 0.87), 0));
            }
        }
    }
}

/// For each port: what can snap onto it, recommended first.
fn snap_section(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, name: &str) {
    col.spawn(k.section("Snap on"));
    let Some(ports) = b.cached_suggestions(name) else {
        col.spawn(k.text("Working out what fits…", 12., SUBTLE, 0));
        return;
    };
    col.spawn(k.text("Parts whose ports fit. Click one to add it next to this part and connect it in one undoable step.", 11., FAINT, 0));
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
                k.text(&p.port, 12.5, TEXT, 2),
                k.text(&p.schema, 11., SUBTLE, 0),
            ]));
            if !p.connected_to.is_empty() {
                card.spawn(k.text(format!("on: {}", p.connected_to.join(", ")), 11., FAINT, 0));
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
                    card.spawn((Node { column_gap: Val::Px(6.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(WARN), k.text(c.conflict.as_deref().unwrap_or(""), 11., SUBTLE, 0)]));
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
fn library_card(col: &mut ChildSpawnerCommands, k: &Kit, b: &Builder, item: &PaletteItem) {
    let cat = category(&item.domain);
    col.spawn(k.text(&item.label, 17., TEXT, 2));
    col.spawn((Node { column_gap: Val::Px(7.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(tag_color(cat)), k.text(format!("{cat}  ·  {}", kind_text(&item.kind)), 11.5, SUBTLE, 0)]));
    col.spawn(Node { margin: UiRect::vertical(Val::Px(8.)), ..wrap() }).with_children(|r| {
        r.spawn(k.button("Place on this level", BuildAction::PlacePreview, Look::Primary, true));
        r.spawn(k.button("Close", BuildAction::ClosePreview, Look::Ghost, true));
    });
    // Snap straight onto the selected part, where a port fits.
    if let Some(name) = b.only_selected() {
        if let Some(ports) = b.cached_suggestions(&name) {
            let fits: Vec<(&str, &sim_system::snap::Candidate)> = ports.iter().filter_map(|p| p.candidates.iter().find(|c| c.kind == item.kind).map(|c| (p.port.as_str(), c))).collect();
            col.spawn(k.section(&format!("Attach to {name}")));
            if fits.is_empty() {
                col.spawn(k.text(format!("No port of {name} fits this part."), 12., SUBTLE, 0));
            }
            for (port, c) in fits {
                col.spawn(wrap()).with_children(|r| {
                    r.spawn(k.button(&format!("{name}.{port}  ←  {}", c.port), BuildAction::AttachPreview(port.to_string()), if c.conflict.is_some() { Look::Ghost } else { Look::Secondary }, c.conflict.is_none()));
                });
                if let Some(conflict) = &c.conflict {
                    col.spawn((Node { column_gap: Val::Px(6.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(WARN), k.text(conflict, 11., SUBTLE, 0)]));
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
                    col.spawn(k.text(n.summary, 13., TEXT, 1));
                    paragraph(col, k, "How it works", n.explanation);
                    equations(col, k, n.equations);
                    paragraph(col, k, "Trade-offs", n.tradeoffs);
                    paragraph(col, k, "Model limits", n.limits);
                }
                None => {
                    col.spawn(k.text("No notes yet for this component: ports and parameters below come straight from the registry.", 12., SUBTLE, 0));
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
                col.spawn(k.text("No datasheet yet. Generate one: sim-system datasheet TYPE --write library/datasheets", 11.5, FAINT, 0));
            }
            if let Some(n) = notes.filter(|n| !n.pairs_with.is_empty()) {
                col.spawn(k.section("Pairs with"));
                col.spawn(wrap()).with_children(|r| {
                    for t in n.pairs_with {
                        let kind = InstanceKind::Element { component_type: t.to_string() };
                        let label = b.element_entry(t).map(|e| e.display_name.clone()).unwrap_or_else(|| t.to_string());
                        r.spawn(k.button(&label, BuildAction::PreviewKind(kind), Look::Chip(false), true));
                    }
                });
            }
        }
        InstanceKind::Subsystem { definition } => {
            let description = b.document.definitions.get(definition).map(|d| d.description.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| item.detail.clone());
            col.spawn(k.text(description, 12.5, Color::srgb(0.80, 0.83, 0.87), 0));
        }
    }
}

/// A generated datasheet: checks, measured values and curve samples.
fn datasheet_section(col: &mut ChildSpawnerCommands, k: &Kit, sheet: &sim_runtime::bench::Datasheet) {
    col.spawn(k.section(&format!("Datasheet · {} bench", sheet.kind)));
    for c in &sheet.checks {
        col.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() }, children![k.dot(if c.passed { OK } else { DANGER }), k.text(format!("{}: {}", c.name, c.detail), 11.5, SUBTLE, 0)]));
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
        col.spawn(k.text(format!("{} ({} vs {}): {}", c.name, c.y_label, c.x_label, sample.join(", ")), 11., FAINT, 0));
    }
}

/// Live traces of the plotted observables, under the viewport.
fn graph_dock(commands: &mut Commands, k: &Kit, b: &Builder) {
    if !b.graphs.visible {
        return;
    }
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(LEFT_WIDTH),
                right: Val::Px(RIGHT_WIDTH),
                bottom: Val::Px(STATUSBAR),
                height: Val::Px(graphs::DOCK),
                padding: UiRect::all(Val::Px(10.)),
                column_gap: Val::Px(10.),
                border: UiRect::top(Val::Px(1.)),
                ..default()
            },
            BackgroundColor(BAR),
            BorderColor::all(BORDER),
            BuilderPanel,
        ))
        .with_children(|dock| {
            if let Some(r) = &b.study.result {
                dock.spawn((Node { border_radius: BorderRadius::top(Val::Px(5.)), position_type: PositionType::Absolute, right: Val::Px(10.), top: Val::Px(-24.), column_gap: Val::Px(10.), padding: UiRect::axes(Val::Px(8.), Val::Px(3.)), align_items: AlignItems::Center, ..default() }, BackgroundColor(BAR)))
                    .with_children(|legend| {
                        legend.spawn(k.text(format!("Study {}", r.name), 11., TEXT, 2));
                        for (label, color) in b.graphs.charts.iter().find(|c| !c.legend.is_empty()).map(|c| c.legend.clone()).unwrap_or_default() {
                            legend.spawn((Node { column_gap: Val::Px(5.), align_items: AlignItems::Center, ..default() }, children![k.dot(Color::srgb_u8(color[0], color[1], color[2])), k.text(label, 11., SUBTLE, 0)]));
                        }
                        legend.spawn(k.button("Back to live", BuildAction::ClearStudy, Look::Ghost, true));
                    });
            }
            if b.graphs.charts.is_empty() {
                dock.spawn(k.text(if b.run.is_none() { "Press Run (R) to record. Select a part to plot its speed, current or torque, or pin quantities from the inspector." } else { "Nothing plottable yet: select a part." }, 12., SUBTLE, 0));
            }
            for (i, c) in b.graphs.charts.iter().enumerate() {
                let [r, g, bl] = c.color;
                let color = Color::srgb_u8(r, g, bl);
                dock.spawn((
                    Node { flex_direction: FlexDirection::Column, flex_grow: 1., flex_basis: Val::Px(0.), min_width: Val::Px(0.), row_gap: Val::Px(3.), ..default() },
                ))
                .with_children(|card| {
                    card.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(6.), flex_shrink: 0., ..default() }).with_children(|head| {
                        head.spawn((Node { column_gap: Val::Px(6.), align_items: AlignItems::Center, min_width: Val::Px(0.), overflow: Overflow::clip(), ..default() }, children![k.dot(color), k.text(&c.title, 11.5, TEXT, 1)]));
                        let latest = c.latest.map(|v| format!("{} {}", num(v), c.unit)).unwrap_or_else(|| "–".into());
                        head.spawn(k.text(latest, 11.5, color, 2));
                        if c.pinned {
                            head.spawn(k.button("×", BuildAction::Unpin(c.id.clone()), Look::Ghost, true));
                        }
                    });
                    if let Some(image) = b.graphs.images.get(i) {
                        card.spawn((
                            Node { flex_grow: 1., border: UiRect::all(Val::Px(1.)), ..default() },
                            BorderColor::all(BORDER),
                            ImageNode::new(image.clone()),
                        ))
                        .with_children(|plot| {
                            let label = |v: f64| format!("{} {}", num(v), c.unit);
                            plot.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(4.), top: Val::Px(2.), ..default() }, children![k.text(label(c.range.1), 10., FAINT, 0)]));
                            plot.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(4.), bottom: Val::Px(2.), ..default() }, children![k.text(label(c.range.0), 10., FAINT, 0)]));
                            let x = if c.x_label == "s" { format!("{:.2} – {:.2} s", c.window.0, c.window.1) } else { format!("{} {} – {}", c.x_label, num(c.window.0), num(c.window.1)) };
                            plot.spawn((Node { position_type: PositionType::Absolute, right: Val::Px(4.), bottom: Val::Px(2.), ..default() }, children![k.text(x, 10., FAINT, 0)]));
                        });
                    }
                });
            }
        });
}

/// Saved studies, the running one, and the latest result's trade-off table.
/// Open another system file in this window: a path field and the system
/// files found under examples/systems-builder, the library and this file's folder.
fn systems_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    body.spawn(k.text("Open another system in this window. Its runs, notes and studies come with it; edits are already saved to this file.", 12., SUBTLE, 0));
    body.spawn(k.section("Open"));
    body.spawn(k.text(&b.document.title, 13., TEXT, 1));
    body.spawn(k.text(b.path().display().to_string(), 11., FAINT, 0));
    let focused = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::OpenSystem);
    let shown = b.input.as_ref().filter(|_| focused).map(|i| i.buffer.clone()).unwrap_or_default();
    body.spawn(Node { margin: UiRect::top(Val::Px(8.)), flex_direction: FlexDirection::Column, flex_shrink: 0., ..default() })
        .with_children(|c| {
            c.spawn(k.input(&shown, "Path to a .system.json file · Enter to open", BuildAction::OpenSystemPath, focused));
        });
    if let Some(pending) = b.open.pending() {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(ACCENT), k.text(format!("Opening {}…", pending.display()), 12., TEXT, 0)]));
        body.spawn(wrap()).with_children(|r| {
            r.spawn(k.button("Cancel", BuildAction::CancelOpen, Look::Danger, true));
        });
    }
    if let Some(e) = b.action_error.as_ref().filter(|e| e.contains("open")) {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, 12., DANGER, 0)]));
    }
    let blockers = b.open_blockers();
    if !blockers.is_empty() {
        body.spawn(k.text(format!("Before opening: {}", blockers.join("; ")), 11.5, WARN, 0));
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
fn actuators_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    body.spawn(Node { margin: UiRect::bottom(Val::Px(6.)), ..wrap() }).with_children(|chips| {
        chips.spawn(k.button("Registry", BuildAction::ActuatorView(calibration::ActuatorView::Registry), Look::Chip(b.actuator_view == calibration::ActuatorView::Registry), true));
        chips.spawn(k.button("Measured evidence", BuildAction::ActuatorView(calibration::ActuatorView::Evidence), Look::Chip(b.actuator_view == calibration::ActuatorView::Evidence), true));
    });
    match b.actuator_view {
        calibration::ActuatorView::Registry => registry_view(body, k, b),
        calibration::ActuatorView::Evidence => calibration::section(body, k, b),
    }
}

fn registry_view(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    let a = &b.actuators;
    body.spawn(k.text("The accepted actuator registry: the single source of measured motor values. Read-only here; families change only by promoting new evidence.", 12., SUBTLE, 0));
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
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(ACCENT), k.text(format!("Loading {}…", pending.display()), 12., TEXT, 0)]));
    }
    if let Some(e) = &a.error {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, 12., DANGER, 0)]));
    }
    let Some(view) = &a.shown else {
        if a.pending().is_none() && a.error.is_none() {
            body.spawn(k.text("Not loaded yet.", 12., SUBTLE, 0));
        }
        return;
    };
    let r = &view.registry;
    if a.error.is_some() {
        body.spawn(k.text(format!("Still showing the last good load: {}", r.path.display()), 11.5, WARN, 1));
    }
    body.spawn(k.text(format!("Showing {}", r.path.display()), 11., FAINT, 0));
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
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, margin: UiRect::top(Val::Px(8.)), flex_shrink: 0., ..default() }, children![k.dot(if current { OK } else { WARN }), k.text(format!("{} · {file}", if current { "Current" } else if check.issue.is_some() { "Not checked" } else { "Stale" }), 12., TEXT, 1)]));
        if let Some(issue) = &check.issue {
            let kind = serde_json::to_value(issue.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            body.spawn(k.text(format!("{kind}: {}", issue.message), 11.5, if kind == "no_robot" { SUBTLE } else { DANGER }, 0));
        }
        for m in &check.models {
            let status = serde_json::to_value(m.status).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let pointer = if m.pointer.is_empty() { "(whole file)" } else { m.pointer.as_str() };
            body.spawn(k.text(format!("{pointer}: {status}"), 11.5, if status == "current" { SUBTLE } else { WARN }, 1));
            if let Some(x) = &m.mismatch {
                let or_none = |v: &Option<String>| v.clone().unwrap_or_else(|| "none".into());
                body.spawn(k.text(format!("motor {} · joint {}", or_none(&x.motor), or_none(&x.joint)), 11., SUBTLE, 0));
                body.spawn(k.text(format!("family {} → accepted {}", or_none(&x.family), or_none(&x.accepted_family)), 11., SUBTLE, 0));
                body.spawn(k.text(format!("have     {}", x.have_hash.as_deref().map(short_hash).unwrap_or_else(|| "none".into())), 11., WARN, 0));
                body.spawn(k.text(format!("accepted {}", x.accepted_hash.as_deref().map(short_hash).unwrap_or_else(|| "none".into())), 11., OK, 0));
            } else if let Some(message) = &m.message {
                body.spawn(k.text(message, 11., SUBTLE, 0));
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
        body.spawn(k.text(format!("Accepted: {}", f.accepted), 11.5, TEXT, 0));
        body.spawn(k.text(&f.description, 11., SUBTLE, 0));
        if !f.limitations.is_empty() {
            body.spawn(k.text("Limitations", 11., FAINT, 2));
            for l in &f.limitations {
                body.spawn(k.text(format!("· {l}"), 11., WARN, 0));
            }
        }
        if !f.has_envelope {
            body.spawn(k.text("No measured envelope.", 11., FAINT, 0));
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
                    top.spawn(k.text(&p.name, 12., TEXT, 1));
                    top.spawn(k.text(format!("{} {}", num(p.value), p.unit), 12., TEXT, 0));
                });
                row.spawn(Node { column_gap: Val::Px(6.), ..default() }).with_children(|bottom| {
                    bottom.spawn(k.text(&p.provenance, 10.5, color, 2));
                    bottom.spawn(k.text(format!("{uncertainty} · {}", p.evidence), 10.5, SUBTLE, 0));
                });
            });
        }
    }
}

fn studies_tab(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    body.spawn(k.text("Run the same system several ways: compare alternatives for a part, or sweep one parameter. Studies are saved in the system file and rerun identically.", 12., SUBTLE, 0));
    body.spawn(k.text("Start one from a part's inspector: Compare alternatives, or Sweep under Parameters.", 11., FAINT, 0));
    if let Some((name, done, total)) = b.study_progress() {
        body.spawn(k.section("Running"));
        body.spawn(k.text(format!("{name}: {done} of {total} variants"), 12.5, TEXT, 1));
        body.spawn((Node { border_radius: BorderRadius::all(Val::Px(3.)), height: Val::Px(6.), flex_shrink: 0., ..default() }, BackgroundColor(RAISED), children![(Node { border_radius: BorderRadius::all(Val::Px(3.)), width: Val::Percent(100. * done as f32 / total.max(1) as f32), ..default() }, BackgroundColor(ACCENT))]));
        body.spawn(wrap()).with_children(|r| {
            r.spawn(k.button("Cancel", BuildAction::CancelStudy, Look::Danger, true));
        });
    }
    body.spawn(k.section(&format!("Saved  {}", b.document.studies.len())));
    if b.document.studies.is_empty() {
        body.spawn(k.text("None yet.", 12., SUBTLE, 0));
    }
    for (name, study) in &b.document.studies {
        let what = match &study.kind {
            sim_system::StudyKind::Compare { alternatives } => format!("compare {} with {} alternative{}", study.instance, alternatives.len(), if alternatives.len() == 1 { "" } else { "s" }),
            sim_system::StudyKind::Sweep { parameter, values } => format!("sweep {}.{parameter} over {} values", study.instance, values.len()),
        };
        body.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(6.), padding: UiRect::vertical(Val::Px(3.)), flex_shrink: 0., ..default() })
            .with_children(|row| {
                row.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1., min_width: Val::Px(0.), ..default() }, children![k.text(name, 12.5, TEXT, 1), k.text(format!("{what} · {} s", num(study.duration)), 11., SUBTLE, 0)]));
                row.spawn(k.button("Run", BuildAction::RunStudy(name.clone()), Look::Secondary, b.study.job.is_none()));
                row.spawn(k.button("×", BuildAction::RemoveStudy(name.clone()), Look::Ghost, true));
            });
    }
    if let Some(e) = &b.study.error {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, 12., DANGER, 0)]));
    }
    body.spawn(k.section(&format!("Runs  {}", b.runs.len())));
    if b.runs.is_empty() {
        body.spawn(k.text("Runs are kept automatically when you restart or edit structure, or with Save run.", 11.5, FAINT, 0));
    }
    for (_, run) in b.runs.iter().take(20) {
        let picked = b.run_picks.contains(&run.id);
        body.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(6.), padding: UiRect::vertical(Val::Px(2.)), flex_shrink: 0., ..default() })
            .with_children(|row| {
                row.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1., min_width: Val::Px(0.), ..default() }, children![
                    k.text(format!("{} · rev {} · {}", run.id, run.revision, run.fidelity), 12., TEXT, 1),
                    k.text(format!("{} s · seed {}{}", num(run.duration), run.seed, if run.note.is_empty() { String::new() } else { format!(" · {}", run.note) }), 11., SUBTLE, 0)
                ]));
                row.spawn(k.button(if picked { "✓" } else { "Pick" }, BuildAction::PickRun(run.id.clone()), Look::Chip(picked), true));
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
            body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(color), k.text(o.headline(), 12., TEXT, 0)]));
            if o.status == "done" {
                body.spawn(k.text(format!("Headless rerun from t = 0 with the recorded document and config ({}, seed {}, {} s); a non-zero difference is a finding about this run.", o.fidelity, o.seed, num(o.duration)), 11., FAINT, 0));
            }
            if o.edited_while_running {
                body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(WARN), k.text("The document was edited while this run recorded: the record keeps the final document, so the replay does not compare a clean run.", 11.5, WARN, 0)]));
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
            card.spawn(k.text(&v.label, 12.5, TEXT, 2));
            if let Some(e) = &v.error {
                card.spawn(k.text(e, 11., DANGER, 0));
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
            BorderColor::all(BORDER),
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

pub(super) fn scroll_panels(mut wheel: MessageReader<MouseWheel>, window: Single<&Window>, mut panels: Query<(&mut ScrollPosition, &Scroll)>) {
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
            position.y = (position.y - delta).max(0.0);
        }
    }
}

pub(crate) fn hover(mut buttons: Query<(&Interaction, &Tint, &mut BackgroundColor), Changed<Interaction>>) {
    for (interaction, tint, mut bg) in &mut buttons {
        bg.0 = match interaction {
            Interaction::Hovered | Interaction::Pressed => tint.hover,
            Interaction::None => tint.idle,
        };
    }
}

fn discussion_header(body:&mut ChildSpawnerCommands,k:&Kit,b:&Builder){
    use discussion::Action as A;
    let action=|label:&str,a:A,look:Look|k.button(label,BuildAction::Discussion(a),look,true);
    let thread=b.discussion.selected.as_ref().and_then(|id|b.document.discussions.threads.get(id));
    if let Some(t)=thread {
        body.spawn(Node{justify_content:JustifyContent::SpaceBetween,align_items:AlignItems::Center,..default()}).with_children(|r|{
            r.spawn(action("‹ All notes",A::List,Look::Ghost));r.spawn(action("More",A::More,Look::Ghost));
        });
        body.spawn(k.text(&t.title,19.,TEXT,2));
        body.spawn(wrap()).with_children(|r|{
            for target in &t.targets {
                if target.missing {r.spawn(k.text(format!("{} · missing",target.label),11.,WARN,0));}
                else {r.spawn(action(&format!("↗ {}",target.path),A::Target(target.path.clone()),Look::Chip(false)));}
            }
        });
        body.spawn(wrap()).with_children(|r|{
            r.spawn(action("Show on model",A::Show("context".into()),Look::Ghost));
            if t.resolved{r.spawn(k.text("Resolved",11.,OK,1));}
            if b.discussion.prior.is_some(){r.spawn(action("Restore view",A::Back,Look::Ghost));}
        });
        agent_card(body,k,b,&t.id);
        if b.discussion.more {
            body.spawn((Node{ border_radius: BorderRadius::all(Val::Px(6.)),flex_direction:FlexDirection::Column,row_gap:Val::Px(4.),padding:UiRect::all(Val::Px(8.)),..default()},BackgroundColor(RAISED))).with_children(|menu|{
                for (label,a) in [("Rename note",A::Title),("Add selected parts",A::LinkSelection),("Inspect linked parts",A::Show("parts".into())),(if t.resolved{"Reopen note"}else{"Mark resolved"},A::Resolve),("Reset marker to part origin",A::Pin)]{menu.spawn(action(label,a,Look::Ghost));}
                menu.spawn(action("Delete note",A::Delete,Look::Danger));
            });
        }
    } else {
        let draft=b.input.as_ref().is_some_and(|i|i.purpose==Purpose::Comment);
        body.spawn(Node{justify_content:JustifyContent::SpaceBetween,align_items:AlignItems::Center,..default()}).with_children(|r|{
            r.spawn(k.text(if draft{"New note"}else{"Notes"},20.,TEXT,2));
            if !draft{r.spawn(k.button("+ Add note",BuildAction::SetMode(Mode::Annotate),Look::Primary,true));}
        });
        if draft {
            body.spawn(k.text("Attached to",11.,SUBTLE,0));
            body.spawn(wrap()).with_children(|r|{for path in &b.discussion.draft_targets{r.spawn(action(&format!("↗ {path}"),A::Target(path.clone()),Look::Chip(false)));}});
        } else {
            body.spawn(Node{justify_content:JustifyContent::SpaceBetween,align_items:AlignItems::Center,..default()}).with_children(|r|{
                r.spawn(action(if b.discussion.open_only{"Open notes"}else{"All notes"},A::OpenOnly,Look::Chip(false)));r.spawn(action("More",A::More,Look::Ghost));
            });
            if b.discussion.more{
                body.spawn(action("Note on selected parts",A::New,Look::Ghost));
                body.spawn(action("Change your name",A::Author,Look::Ghost));
                body.spawn(action(if b.discussion.selected_only{"Show all parts"}else{"Only selected parts"},A::SelectedOnly,Look::Ghost));
                body.spawn(k.button("Import existing notes",BuildAction::ImportNotes,Look::Ghost,true));
            }
            body.spawn(k.button(if b.agent.state.auto_answer {"Auto-answer: on"} else {"Auto-answer: off"},BuildAction::Agent(agent::Request::Configure{auto_answer:!b.agent.state.auto_answer}),Look::Chip(b.agent.state.auto_answer),b.agent.state.ready));
            if let Some(e)=&b.agent.state.error{body.spawn(k.text(e,11.,WARN,0));}
            body.spawn(k.text(if b.mode==Mode::Annotate{"Click a surface on the model to start a note."}else{"Click a pin on the model to join its conversation."},12.,SUBTLE,0));
        }
    }
}

/// System discussions drawn with the shared annotation views.
struct NotesHost<'a> { b: &'a Builder }
impl crate::annotate::Host<sim_system::display::Target> for NotesHost<'_> {
    type Action = BuildAction;
    fn open(&self, thread: &str) -> BuildAction { BuildAction::Discussion(discussion::Action::Open(thread.into())) }
    fn menu(&self, comment: &str) -> BuildAction { BuildAction::Discussion(discussion::Action::CommentMore(comment.into())) }
    fn edit(&self, comment: &str) -> BuildAction { BuildAction::Discussion(discussion::Action::Edit(comment.into())) }
    fn delete(&self, comment: &str) -> BuildAction { BuildAction::Discussion(discussion::Action::DeleteComment(comment.into())) }
    fn anchor(&self, t: &sim_system::display::Target) -> Option<BuildAction> { Some(BuildAction::Discussion(discussion::Action::Target(t.path.clone()))) }
    fn anchor_text(&self, t: &sim_system::display::Target) -> String { t.path.clone() }
    fn link(&self, c: &sim_system::display::Comment, link: &sim_markdown::Link) -> Option<BuildAction> {
        if let Some(path)=link.target.strip_prefix("part:").or_else(||link.target.strip_prefix("group:")){
            c.links.iter().find(|t|!t.missing&&(t.path==path||t.label==path.rsplit('/').next().unwrap_or(path))).map(|t|BuildAction::Discussion(discussion::Action::Target(t.path.clone())))
        }else if link.target.starts_with("https://")||link.target.starts_with("http://")||sim_markdown::source_location(&link.target).is_ok(){Some(BuildAction::OpenReference(link.target.clone()))}else{None}
    }
    fn badge(&self, thread: &str) -> Option<String> { self.b.agent_badge(thread) }
}

fn discussion_content(body:&mut ChildSpawnerCommands,k:&Kit,b:&Builder){
    let host=NotesHost{b};
    if let Some(t)=b.discussion.selected.as_ref().and_then(|id|b.document.discussions.threads.get(id)){
        crate::annotate::messages(body,k,&host,t,b.discussion.comment_menu.as_deref());
    }else if b.input.as_ref().is_some_and(|i|i.purpose==Purpose::Comment){
        body.spawn(k.text("What would you like to discuss?",14.,SUBTLE,0));
    }else{
        let paths:Vec<_>=b.selected.iter().map(|n|b.full_path(n)).collect();
        let shown=b.document.discussions.threads.values().filter(|t|(!b.discussion.open_only||!t.resolved)&&(!b.discussion.selected_only||t.targets.iter().any(|r|paths.iter().any(|p|p==&r.path||r.path.starts_with(&format!("{p}/"))))));
        let count=crate::annotate::list(body,k,&host,shown);
        if count==0{body.spawn(k.text("No notes here yet",16.,TEXT,1));body.spawn(k.text("Add a note, then click the part you want to talk about.",13.,SUBTLE,0));}
    }
}

fn discussion_composer(body:&mut ChildSpawnerCommands,k:&Kit,b:&Builder){
    use discussion::Action as A;
    let input=b.input.as_ref().filter(|i|matches!(i.purpose,Purpose::Comment|Purpose::CommentAuthor|Purpose::ThreadTitle));
    let focused=input.is_some();
    let special=input.is_some_and(|i|i.purpose!=Purpose::Comment);
    let shown=input.map(|i|i.buffer.as_str()).unwrap_or("");
    let label=if input.is_some_and(|i|i.purpose==Purpose::CommentAuthor){"Your name"}else if input.is_some_and(|i|i.purpose==Purpose::ThreadTitle){"Note title"}else if b.discussion.editing.is_some(){"Edit message"}else if b.discussion.selected.is_none(){"Write a note"}else{"Reply"};
    body.spawn(k.text(label,12.,SUBTLE,1));
    // A persistent footer keeps the reply field in reach while messages scroll.
    body.spawn((Button,BuildAction::Discussion(A::Reply),Tint{idle:RAISED,hover:HOVER_BG},Node{ border_radius: BorderRadius::all(Val::Px(7.)),min_height:Val::Px(if special{36.}else{76.}),max_height:Val::Px(180.),overflow:Overflow::clip(),padding:UiRect::all(Val::Px(10.)),border:UiRect::all(Val::Px(1.)),..default()},BackgroundColor(RAISED),BorderColor::all(if focused{ACCENT}else{BORDER}))).with_children(|field|{
        field.spawn(k.text(if focused{format!("{shown}|")}else{"Write a reply…".into()},14.,if focused{TEXT}else{FAINT},0));
    });
    body.spawn(Node{justify_content:JustifyContent::SpaceBetween,align_items:AlignItems::Center,..default()}).with_children(|r|{
        if focused {r.spawn(k.button("Cancel",BuildAction::Discussion(A::CancelDraft),Look::Ghost,true));}
        else{r.spawn(k.button(&b.discussion.author,BuildAction::Discussion(A::Author),Look::Ghost,true));}
        if focused{r.spawn(k.button(if special||b.discussion.editing.is_some(){"Save"}else if b.discussion.selected.is_none(){"Post note"}else{"Post reply"},BuildAction::Discussion(A::Submit),Look::Primary,!shown.trim().is_empty()));}
    });
    if let Some(error)=&b.discussion.error{body.spawn(k.text(error,11.,WARN,0));}
    if focused{body.spawn(k.text("Enter to post · Shift+Enter for a new line",10.5,FAINT,0));}
}

fn agent_card(body:&mut ChildSpawnerCommands,k:&Kit,b:&Builder,id:&str){
    use agent::Request as A;
    use sim_agent::Status;
    let button=|label:&str,a:A|k.button(label,BuildAction::Agent(a),Look::Ghost,b.agent.state.ready);
    let run=b.agent.state.latest(id);
    body.spawn((Node{ border_radius: BorderRadius::all(Val::Px(6.)),flex_direction:FlexDirection::Column,row_gap:Val::Px(5.),padding:UiRect::all(Val::Px(9.)),flex_shrink:0.,min_width:Val::Px(0.),max_width:Val::Percent(100.),overflow:Overflow::clip(),..default()},BackgroundColor(RAISED))).with_children(|card|{
        card.spawn(Node{justify_content:JustifyContent::SpaceBetween,align_items:AlignItems::Center,..default()}).with_children(|row|{
            row.spawn(k.text("Codex · Astra / High",12.,TEXT,1));
            if let Some(r)=run.filter(|r|r.status.active()) {
                row.spawn(button("Stop",A::Cancel{run:r.id.clone()}));
            }else{
                row.spawn(button("Ask Codex",A::Ask{discussion:id.into(),question:None,request_id:None}));
            }
        });
        if let Some(error)=&b.agent.state.error{card.spawn(k.text(error,11.,WARN,0));}
        if let Some(r)=run{
            card.spawn(k.text(if r.status.active(){format!("{} · {}s",r.activity,sim_agent::now().saturating_sub(r.created_at))}else{r.activity.clone()},11.5,if r.status==Status::Failed{WARN}else{SUBTLE},0));
            if let Some(error)=&r.error{card.spawn(k.text(error,11.,WARN,0));}
            card.spawn(wrap()).with_children(|row|{
                row.spawn(button(if b.agent.expanded{"Hide activity"}else{"Show activity"},A::Activity));
                if matches!(r.status,Status::Failed|Status::Cancelled){row.spawn(button("Retry",A::Retry{run:r.id.clone()}));}
            });
            if let Some(count)=r.input.context["context_summary"]["scope"]["included_instances"].as_u64(){
                let resolved=r.input.context["context_summary"]["resolved"].as_bool().unwrap_or(false);
                card.spawn(k.text(format!("Context: {count} parts & groups · {}",if resolved{"model resolved"}else{"source only; model has errors"}),10.5,SUBTLE,0));
            }
            if b.agent.expanded{
                card.spawn(k.text(format!("Source revision {} · started {}",r.input.revision,sim_system::display::relative_time(&r.created_at.to_string())),10.,FAINT,0));
                for e in b.agent.state.events.iter().filter(|e|e.run==r.id).rev().take(3){
                    let mut message=e.message.chars().take(150).collect::<String>();if e.message.chars().count()>150{message.push('…');}
                    let mut text=k.text(message,11.,SUBTLE,0);text.3=TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter);
                    card.spawn((text,Node{min_width:Val::Px(0.),max_width:Val::Percent(100.),..default()}));
                }
            }
        }else{card.spawn(k.text("Ask about this note and its linked parts.",11.5,SUBTLE,0));}
    });
}

pub(crate) fn markdown_theme(k:&Kit)->crate::markdown::Theme{crate::markdown::Theme{regular:k.f.regular.clone(),strong:k.f.semibold.clone(),italic:k.f.italic.clone(),mono:k.f.mono.clone(),text:TEXT,muted:SUBTLE,accent:ACCENT,code:Color::srgb(0.91,0.75,0.49),surface:RAISED}}
fn source_preview(col:&mut ChildSpawnerCommands,k:&Kit,b:&Builder){
    col.spawn(k.button("‹ Back to inspector",BuildAction::CloseReference,Look::Ghost,true));
    col.spawn(k.text("Source reference",17.,TEXT,2));
    match &b.reference.result{
        None=>{col.spawn(k.text("Reading source…",12.,SUBTLE,0));},
        Some(Err(e))=>{col.spawn(k.text(e,12.,WARN,0));},
        Some(Ok(source))=>{
            col.spawn(k.text(format!("{}:{}",source.path,source.line),12.,ACCENT,1));
            col.spawn(k.text("Read-only · current file",10.5,FAINT,0));
            for line in &source.lines{
                col.spawn((Node{width:Val::Percent(100.),min_width:Val::Px(0.),padding:UiRect::axes(Val::Px(5.),Val::Px(3.)),flex_shrink:0.,overflow:Overflow::clip(),..default()},BackgroundColor(if line.focused{RAISED}else{Color::NONE}))).with_children(|row|{
                    row.spawn((Text::new(format!("{:>3}  {}",line.number,line.text)),TextFont{font:k.f.mono.clone().into(),font_size:FontSize::Px(11.),..default()},TextColor(if line.focused{ACCENT}else{SUBTLE}),TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),Node{width:Val::Percent(100.),min_width:Val::Px(0.),..default()}));
                });
            }
        }
    }
}
