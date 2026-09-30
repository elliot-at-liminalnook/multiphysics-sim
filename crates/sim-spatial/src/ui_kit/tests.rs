use super::*;

/// Feature drawings whose colours are their own, not UI chrome: a colour
/// there may equal a token by coincidence without being a second
/// definition of it. (Tint literals are forbidden everywhere outside the kit.)
const COLOUR_ALLOWLIST: [&str; 5] = [
    // The schematic canvas: a light drawing surface with its own ink.
    "builder/schematic.rs",
    // 3D placement previews and gizmos.
    "builder/placement.rs",
    // 3D model and scene materials.
    "models.rs",
    "linked.rs",
    "animation.rs",
];

/// The kit's colour tokens, as sRGB components.
fn tokens() -> Vec<(&'static str, [f32; 3])> {
    [
        ("BAR", BAR),
        ("SURFACE", SURFACE),
        ("RAISED", RAISED),
        ("HOVER_BG", HOVER_BG),
        ("BORDER", BORDER),
        ("TEXT", TEXT),
        ("SUBTLE", SUBTLE),
        ("FAINT", FAINT),
        ("VALUE", VALUE),
        ("ACCENT", ACCENT),
        ("ACCENT_HOVER", ACCENT_HOVER),
        ("ACCENT_BG", ACCENT_BG),
        ("ON_ACCENT", ON_ACCENT),
        ("WARN", WARN),
        ("DANGER", DANGER),
        ("DANGER_HOVER", DANGER_HOVER),
        ("DANGER_EDGE", DANGER_EDGE),
        ("OK", OK),
    ]
    .into_iter()
    .map(|(name, colour)| {
        let c = colour.to_srgba();
        (name, [c.red, c.green, c.blue])
    })
    .collect()
}

/// The three numbers of every `Color::srgb(r, g, b)` literal on a line.
fn srgb_literals(line: &str, needle: &str) -> Vec<[f32; 3]> {
    let mut found = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find(needle) {
        rest = &rest[at + needle.len()..];
        let Some(end) = rest.find(')') else { break };
        let parts: Vec<Option<f32>> = rest[..end].split(',').map(|p| p.trim().trim_end_matches("f32").trim_end_matches('_').parse::<f32>().ok()).collect();
        if let [Some(r), Some(g), Some(b)] = parts[..] {
            found.push([r, g, b]);
        }
    }
    found
}

/// Tokens are defined once (ui_kit/theme.rs): outside the kit there is no
/// `Tint` struct literal (use a preset or `Tint::new`) and no
/// `Color::srgb` literal equal to a token, except in the commented
/// allowlist of feature drawings. Names every offending file and line.
#[test]
fn ui_colours_come_from_the_kit() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let kit = src.join("ui_kit");
    // Built so this file never contains the needles literally.
    let tint = ["Tint", " {"].concat();
    let tint_tight = ["Tint", "{"].concat();
    let srgb = ["Color::", "srgb("].concat();
    let tokens = tokens();
    let mut offenders = Vec::new();
    let mut dirs = vec![src.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path != kit {
                    dirs.push(path);
                }
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let relative = path.strip_prefix(&src).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let allowed = COLOUR_ALLOWLIST.contains(&relative.as_str());
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            for (i, line) in text.lines().enumerate() {
                if line.contains(tint.as_str()) || line.contains(tint_tight.as_str()) {
                    offenders.push(format!("{relative}:{}: Tint literal: {}", i + 1, line.trim()));
                }
                if allowed {
                    continue;
                }
                for rgb in srgb_literals(line, &srgb) {
                    if let Some((name, _)) = tokens.iter().find(|(_, t)| t.iter().zip(rgb).all(|(a, b)| (a - b).abs() < 1e-6)) {
                        offenders.push(format!("{relative}:{}: colour literal equal to ui_kit::{name}: {}", i + 1, line.trim()));
                    }
                }
            }
        }
    }
    assert!(offenders.is_empty(), "use the ui_kit tokens and Tint presets, not literals:\n{}", offenders.join("\n"));
}

#[test]
fn srgb_literals_are_read_from_a_line() {
    let line = "a(Color::srgb(0.1, 0.2, 0.3)), b(Color::srgb(1.0,0.5 , 0.25f32)), c(Color::srgb(x, 0.1, 0.2))";
    assert_eq!(srgb_literals(line, "Color::srgb("), vec![[0.1, 0.2, 0.3], [1.0, 0.5, 0.25]]);
}

/// Every look paints; a disabled button never lights on hover and its
/// label is FAINT; the current tab is the only underlined look.
#[test]
fn looks_paint_the_builder_palette() {
    let all = [Look::Primary, Look::Secondary, Look::Ghost, Look::Danger, Look::Tab(true), Look::Tab(false), Look::Chip(true), Look::Chip(false), Look::Segment(true), Look::Segment(false)];
    for look in all {
        let off = look.paint(false);
        assert_eq!(off.hover, off.idle, "{look:?}");
        assert_eq!(off.text, FAINT, "{look:?}");
        assert_eq!(look.paint(true).underline, look == Look::Tab(true));
    }
    let p = Look::Secondary.paint(true);
    assert_eq!((p.idle, p.hover, p.text, p.border), (RAISED, HOVER_BG, TEXT, BORDER));
    let p = Look::Primary.paint(true);
    assert_eq!((p.idle, p.hover, p.text), (ACCENT, ACCENT_HOVER, ON_ACCENT));
    assert_eq!(Tint::selectable(true), Tint::new(ACCENT_BG, ACCENT_BG));
    assert_eq!(Tint::selectable(false), Tint::CLEAR);
}

/// Kit bundles spawn (a duplicate component in a bundle panics at spawn,
/// which no compile step catches), and the slider starts where it is told.
#[test]
fn kit_bundles_spawn() {
    #[derive(Component)]
    struct Marker;
    let fonts = UiFonts { regular: default(), italic: default(), mono: default(), icons: Default::default(), medium: default(), semibold: default() };
    let k = Kit::new(&fonts);
    let mut world = World::new();
    let slider = world.spawn(k.slider(SliderLook::Timebar, 0.25, Marker, "Timeline")).id();
    assert_eq!(world.get::<bevy::ui_widgets::SliderValue>(slider).map(|v| v.0), Some(0.25));
    world.spawn(k.pointer_surface("Chart", false));
    world.spawn(k.button("Run", Marker, Look::Primary, true));
    world.spawn(k.chip("Parts", Marker, true, true));
    world.spawn(k.list_item("missing-icon", ACCENT, "Title", "Subtitle", Marker, true));
    world.spawn(k.input("", "Type", Marker, false));
    world.spawn(k.section("Section"));
    world.spawn(k.dock(Dock::Top { height: TOPBAR }, Node::default()));
    world.spawn(k.scroll_area(Node::default(), 12.0));
    world.spawn(k.chart_image(Handle::default(), Node::default(), true));
    world.spawn(k.chart_label("1 s".to_string(), Corner::TopLeft));
    assert!(!slider_held(true, &Interaction::Hovered) && slider_held(true, &Interaction::Pressed) && !slider_held(false, &Interaction::Pressed));
}
