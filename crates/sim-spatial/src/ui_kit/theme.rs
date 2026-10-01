//! The kit's tokens, defined once: layout metrics, the palette, the type
//! scale, the interface fonts, and the two styling components (`Look` for
//! buttons, `Tint` for other clickable surfaces). Values are the System
//! Builder's (the look every mode now shares).
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use std::collections::BTreeMap;

// Layout: the builder's chrome (toolbar, status bar, side columns).
pub(crate) const TOPBAR: f32 = 52.0;
pub(crate) const STATUSBAR: f32 = 30.0;
pub(crate) const LEFT_WIDTH: f32 = 300.0;
pub(crate) const RIGHT_WIDTH: f32 = 330.0;
/// The mode switcher's strip along the window's bottom edge
/// (window-first-usability): reserved once here. Every `Dock` ends above
/// it (`Kit::dock` adds it), `Dock::Strip` is the strip itself, and any
/// other bottom-anchored node offsets by it ([`above_strip`]), so nothing
/// a mode draws lies under the switcher or its message.
pub(crate) const SWITCHER_STRIP: f32 = 40.0;

/// A bottom offset `px` above the switcher strip (for bottom-anchored
/// nodes that are not docks: floating bars, toasts, pages).
pub(crate) fn above_strip(px: f32) -> Val {
    Val::Px(px + SWITCHER_STRIP)
}

/// The `GlobalZIndex` of a modal backdrop (`Kit::backdrop`): above every
/// panel, the switcher strip (40) and the radial pie (45).
pub(crate) const MODAL_Z: i32 = 50;

// Palette: dark neutral surfaces, one accent, semantic warning/danger/ok.
pub(crate) const BAR: Color = Color::srgb(0.071, 0.086, 0.106);
pub(crate) const SURFACE: Color = Color::srgb(0.094, 0.110, 0.137);
pub(crate) const RAISED: Color = Color::srgb(0.129, 0.149, 0.180);
pub(crate) const HOVER_BG: Color = Color::srgb(0.161, 0.184, 0.220);
pub(crate) const BORDER: Color = Color::srgb(0.180, 0.208, 0.247);
pub(crate) const TEXT: Color = Color::srgb(0.902, 0.922, 0.945);
pub(crate) const SUBTLE: Color = Color::srgb(0.600, 0.651, 0.710);
pub(crate) const FAINT: Color = Color::srgb(0.420, 0.463, 0.522);
pub(crate) const ACCENT: Color = Color::srgb(0.30, 0.83, 0.75);
/// The primary button's hover (a lighter accent).
pub(crate) const ACCENT_HOVER: Color = Color::srgb(0.40, 0.90, 0.82);
pub(crate) const ACCENT_BG: Color = Color::srgb(0.098, 0.251, 0.239);
pub(crate) const ON_ACCENT: Color = Color::srgb(0.035, 0.090, 0.086);
pub(crate) const WARN: Color = Color::srgb(0.949, 0.749, 0.388);
pub(crate) const DANGER: Color = Color::srgb(0.937, 0.463, 0.435);
/// The danger button's hover fill and idle border.
pub(crate) const DANGER_HOVER: Color = Color::srgb(0.25, 0.12, 0.12);
pub(crate) const DANGER_EDGE: Color = Color::srgb(0.42, 0.20, 0.20);
pub(crate) const OK: Color = Color::srgb(0.435, 0.816, 0.557);
/// A property's read-only value (between TEXT and SUBTLE).
pub(crate) const VALUE: Color = Color::srgb(0.84, 0.87, 0.91);

/// Type scale (logical px). Weights: 0 regular, 1 medium, 2 semibold.
pub(crate) mod size {
    /// Panel title ("Nothing selected", an instance's name).
    pub const TITLE: f32 = 16.0;
    /// The toolbar's product name.
    pub const PRODUCT: f32 = 14.0;
    /// List item title.
    pub const ITEM: f32 = 13.0;
    /// Buttons, property rows, text entry.
    pub const BODY: f32 = 12.5;
    /// Descriptions and status text.
    pub const SMALL: f32 = 12.0;
    /// Chips, tabs, notes, status-bar counts.
    pub const CAPTION: f32 = 11.5;
    /// List item subtitle.
    pub const DETAIL: f32 = 11.0;
    /// Section headings (upper case).
    pub const SECTION: f32 = 10.5;
}

/// The interface fonts and icons: added to the asset stores once while the
/// app is built (`app::CorePlugin`), since the first mode's OnEnter runs
/// before Startup and spawns text with them. Shared by every mode.
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
    pub(crate) fn load(world: &mut World) -> Self {
        let icons = {
            let mut images = world.resource_mut::<Assets<Image>>();
            sim_core::icons::NAMES
                .iter()
                .map(|name| {
                    let image = Image::new(
                        bevy::render::render_resource::Extent3d { width: 48, height: 48, depth_or_array_layers: 1 },
                        bevy::render::render_resource::TextureDimension::D2,
                        sim_core::icons::rgba(name, 48),
                        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                        RenderAssetUsages::default(),
                    );
                    (name.to_string(), images.add(image))
                })
                .collect()
        };
        let mut fonts = world.resource_mut::<Assets<Font>>();
        let mut load = |bytes: &[u8]| fonts.add(Font::from_bytes(bytes.to_vec()));
        UiFonts {
            icons,
            italic: load(include_bytes!("../../assets/fonts/IBMPlexSans-Italic.ttf")),
            mono: load(include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf")),
            regular: load(include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf")),
            medium: load(include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf")),
            semibold: load(include_bytes!("../../assets/fonts/IBMPlexSans-SemiBold.ttf")),
        }
    }
    /// The face for a weight: 0 regular, 1 medium, else semibold.
    pub(crate) fn weight(&self, weight: u8) -> Handle<Font> {
        match weight {
            0 => self.regular.clone(),
            1 => self.medium.clone(),
            _ => self.semibold.clone(),
        }
    }
}

/// Idle and hovered background of a clickable surface that is not a kit
/// button (list rows, cards, fields); repainted by `ui_kit::repaint_tints`
/// when the pointer or the tint changes. Build one with a preset or
/// [`Tint::new`], not a struct literal (the source guard checks this).
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(crate) struct Tint {
    pub(crate) idle: Color,
    pub(crate) hover: Color,
}

impl Tint {
    pub(crate) const fn new(idle: Color, hover: Color) -> Self {
        Tint { idle, hover }
    }
    /// Transparent, lit on hover (rows, links).
    pub(crate) const CLEAR: Tint = Tint::new(Color::NONE, HOVER_BG);
    /// A raised card or field, lit on hover.
    pub(crate) const RAISED: Tint = Tint::new(RAISED, HOVER_BG);
    /// The panel surface, lit on hover.
    pub(crate) const SURFACE: Tint = Tint::new(SURFACE, HOVER_BG);
    /// A selectable row: accent fill while selected (no hover change), else clear.
    pub(crate) const fn selectable(selected: bool) -> Tint {
        if selected { Tint::new(ACCENT_BG, ACCENT_BG) } else { Tint::CLEAR }
    }
}

/// A kit button's look. It is a component on the button, so a mode that
/// toggles a button's state (a lit segment, the current tab) writes a new
/// `Look` and `ui_kit::repaint_buttons` restyles it, hover included.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Look {
    /// The one main action of a panel (Run): accent fill.
    Primary,
    /// An ordinary action: raised, bordered.
    Secondary,
    /// A quiet action: text only until hovered.
    Ghost,
    /// A destructive action: danger text and edge.
    Danger,
    /// A tab of a tab strip; `true` is the current tab (underlined).
    Tab(bool),
    /// A toggle chip (rounded pill); `true` is on.
    Chip(bool),
    /// One option of a segmented control; `true` is chosen.
    Segment(bool),
}

/// Everything a `Look` decides, for enabled or disabled buttons.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Paint {
    pub idle: Color,
    pub hover: Color,
    pub text: Color,
    pub border: Color,
    pub weight: u8,
    pub size: f32,
    pub pad: (f32, f32),
    pub radius: f32,
    /// The current tab: a 2 px bottom border instead of a 1 px frame.
    pub underline: bool,
}

impl Look {
    pub(crate) fn paint(self, enabled: bool) -> Paint {
        let (idle, hover, text, border, weight) = match self {
            Look::Primary => (ACCENT, ACCENT_HOVER, ON_ACCENT, ACCENT, 2),
            Look::Secondary => (RAISED, HOVER_BG, TEXT, BORDER, 1),
            Look::Ghost => (Color::NONE, HOVER_BG, SUBTLE, Color::NONE, 1),
            Look::Danger => (Color::NONE, DANGER_HOVER, DANGER, DANGER_EDGE, 1),
            Look::Tab(on) => (Color::NONE, if on { Color::NONE } else { HOVER_BG }, if on { TEXT } else { SUBTLE }, Color::NONE, if on { 2 } else { 1 }),
            Look::Chip(on) => (if on { ACCENT_BG } else { RAISED }, if on { ACCENT_BG } else { HOVER_BG }, if on { ACCENT } else { SUBTLE }, if on { ACCENT } else { BORDER }, 1),
            Look::Segment(on) => (if on { ACCENT_BG } else { Color::NONE }, if on { ACCENT_BG } else { HOVER_BG }, if on { ACCENT } else { SUBTLE }, Color::NONE, 1),
        };
        let (hover, text) = if enabled { (hover, text) } else { (idle, FAINT) };
        let (pad, size) = match self {
            Look::Chip(_) => ((9., 3.), size::CAPTION),
            Look::Tab(_) => ((3., 10.), size::CAPTION),
            _ => ((11., 5.), size::BODY),
        };
        let underline = matches!(self, Look::Tab(true));
        let radius = if underline {
            0.
        } else if matches!(self, Look::Chip(_)) {
            10.
        } else {
            5.
        };
        Paint { idle, hover, text, border: if underline { ACCENT } else { border }, weight, size, pad, radius, underline }
    }
}
