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

/// The pie picks as RoboCAD's `_index_at`: the first entry straight up,
/// then clockwise (y down), sectors centred on their entries, nothing in
/// the dead centre.
#[test]
fn pie_picks_the_entry_under_the_pointer() {
    use pie::{DEAD, RADIUS, index_at, slot};
    let c = Vec2::new(300.0, 200.0);
    // `deg` clockwise from straight up, 50 px out.
    let at = |deg: f32| c + 50.0 * Vec2::new(deg.to_radians().sin(), -deg.to_radians().cos());
    assert_eq!(index_at(c, c + Vec2::new(0.0, -50.0), 4), Some(0));
    assert_eq!(index_at(c, c + Vec2::new(50.0, 0.0), 4), Some(1));
    assert_eq!(index_at(c, c + Vec2::new(0.0, 50.0), 4), Some(2));
    assert_eq!(index_at(c, c + Vec2::new(-50.0, 0.0), 4), Some(3));
    assert_eq!(index_at(c, c, 4), None);
    assert_eq!(index_at(c, c + Vec2::new(DEAD - 0.5, 0.0), 4), None);
    assert_eq!(index_at(c, c + Vec2::new(DEAD + 0.5, 0.0), 4), Some(1));
    assert_eq!(index_at(c, at(90.0), 0), None);
    // Eight sectors of 45°, boundaries at 22.5° either side of each entry.
    assert_eq!(index_at(c, at(22.0), 8), Some(0));
    assert_eq!(index_at(c, at(23.0), 8), Some(1));
    assert_eq!(index_at(c, at(337.0), 8), Some(7));
    assert_eq!(index_at(c, at(338.0), 8), Some(0));
    assert_eq!(index_at(c, at(180.0), 8), Some(4));
    // Slots: the first straight up, the second (of four) to the right.
    for n in [1, 3, 4, 8] {
        assert!(slot(0, n).distance(Vec2::new(0.0, -RADIUS)) < 1e-3, "{n}");
    }
    assert!(slot(1, 4).distance(Vec2::new(RADIUS, 0.0)) < 1e-3);
    // Each slot is picked by a pointer at it.
    for i in 0..8 {
        assert_eq!(index_at(c, c + slot(i, 8), 8), Some(i));
    }
}

fn command(id: &str, label: &str, category: &str, keys: &[&str]) -> palette::PaletteEntry {
    palette::PaletteEntry { id: id.into(), label: label.into(), category: category.into(), keys: keys.iter().map(|k| k.to_string()).collect(), note: String::new(), enabled: true }
}

/// The palette ranks as RoboCAD's `CommandPalette.refresh`.
#[test]
fn palette_ranks_as_robocad() {
    use palette::{SHOWN, rank, score};
    let entries = vec![command("cad:chordal", "Chordal fillet", "Modify", &[]), command("cad:fillet", "Fillet", "Modify", &["Ctrl+F"]), command("cad:shell", "Shell", "Feature", &[])];
    // No query: everything, by label, score 0.
    let all = rank(&entries, "");
    assert_eq!(all.iter().map(|r| r.index).collect::<Vec<_>>(), vec![0, 1, 2]);
    assert!(entries.iter().all(|e| score(e, "") == Some(0)));
    // A label match scores 1 + its position: "Fillet" (1) before "Chordal fillet" (9).
    assert_eq!(score(&entries[1], "fil"), Some(1));
    assert_eq!(score(&entries[0], "fil"), Some(9));
    let fil = rank(&entries, "  FIL ");
    assert_eq!(fil.iter().map(|r| r.index).collect::<Vec<_>>(), vec![1, 0], "Shell has no 'i' anywhere, so it is left out");
    assert_eq!(fil[0].text, "Modify: Fillet    [Ctrl+F]");
    assert_eq!(fil[1].text, "Modify: Chordal fillet");
    assert_eq!(fil[0].conflict, None);
    // Every character somewhere in "id label category", in any order: 50.
    let hole = command("cad:hole", "Make hole", "Feature", &[]);
    assert_eq!(score(&hole, "hm"), Some(50));
    assert_eq!(score(&hole, "mh"), Some(50));
    assert_eq!(score(&hole, "zz"), None);
    // A note follows the label.
    let mut noted = command("cad:x", "Export", "File", &["Ctrl+E"]);
    noted.note = "GUI-only".into();
    assert_eq!(rank(&[noted], "")[0].text, "File: Export  (GUI-only)    [Ctrl+E]");
    // At most 60 rows, in label order.
    let many: Vec<_> = (0..70).rev().map(|i| command(&format!("c{i}"), &format!("Command {i:02}"), "Test", &[])).collect();
    let shown = rank(&many, "");
    assert_eq!(shown.len(), SHOWN);
    assert_eq!(many[shown[0].index].label, "Command 00");
    assert_eq!(many[shown[59].index].label, "Command 59");
}

/// Keys bound twice (compared lower-cased) warn on both entries, naming the other.
#[test]
fn palette_warns_of_key_conflicts() {
    use palette::{conflicts, rank};
    let entries = vec![command("cad:mirror", "Mirror", "Modify", &["Ctrl+Shift+M"]), command("cad:measure", "Measure", "Inspect", &["ctrl+shift+m"]), command("cad:move", "Move", "Edit", &["G"])];
    let c = conflicts(&entries);
    assert_eq!(c.len(), 1);
    assert_eq!(c.get("ctrl+shift+m"), Some(&vec![0, 1]));
    let rows = rank(&entries, "");
    assert_eq!(rows.iter().map(|r| r.index).collect::<Vec<_>>(), vec![1, 0, 2]);
    assert_eq!(rows[0].conflict.as_deref(), Some("Mirror"));
    assert_eq!(rows[0].text, "Inspect: Measure    [ctrl+shift+m]  \u{26a0} conflicts with Mirror");
    assert_eq!(rows[1].conflict.as_deref(), Some("Measure"));
    assert!(rows[1].text.ends_with("  \u{26a0} conflicts with Measure"));
    assert_eq!(rows[2].conflict, None);
    assert_eq!(rows[2].text, "Edit: Move    [G]");
}

/// Form fields read numbers as the CAD numeric bar does, then RoboCAD's ranges.
#[test]
fn form_fields_evaluate() {
    use form::{FieldKind, FieldValue, Unit, evaluate};
    let number = |unit: Unit| FieldKind::Number { unit, min: None, max: None, decimals: 3 };
    let close = |r: Result<FieldValue, String>, want: f64| matches!(r, Ok(FieldValue::Number(v)) if (v - want).abs() < 1e-9);
    assert!(close(evaluate(&number(Unit::Length), "20mm + 0.3"), 20.3));
    assert!(close(evaluate(&number(Unit::Length), "1in"), 25.4));
    assert!(close(evaluate(&number(Unit::Angle), "45deg"), 45.0));
    assert!(close(evaluate(&number(Unit::Angle), "pi rad"), 180.0));
    assert!(close(evaluate(&number(Unit::Count), "50/2"), 25.0));
    assert!(evaluate(&number(Unit::Length), "2 +").is_err());
    let count = FieldKind::Number { unit: Unit::Count, min: Some(1.0), max: Some(500.0), decimals: 0 };
    assert_eq!(evaluate(&count, "3"), Ok(FieldValue::Number(3.0)));
    let half = evaluate(&count, "2.5").unwrap_err();
    assert!(half.contains("whole number") && half.contains("2.5"), "{half}");
    let radius = FieldKind::Number { unit: Unit::Length, min: Some(0.01), max: Some(100.0), decimals: 2 };
    assert_eq!(evaluate(&radius, "0"), Err("0 is outside 0.01\u{2026}100".to_string()));
    assert_eq!(evaluate(&radius, "101"), Err("101 is outside 0.01\u{2026}100".to_string()));
    assert_eq!(evaluate(&radius, "100"), Ok(FieldValue::Number(100.0)));
    // Not rounded to the dialog's decimals: the typed value is kept.
    assert_eq!(evaluate(&radius, "1.2345"), Ok(FieldValue::Number(1.2345)));
    let vector = FieldKind::Vector { unit: Unit::Length };
    assert_eq!(evaluate(&vector, "1, 2, 3"), Ok(FieldValue::Vector([1.0, 2.0, 3.0])));
    assert!(evaluate(&vector, "1, 2").is_err());
    assert!(evaluate(&vector, "1, 2 +, 3").unwrap_err().starts_with("y: "));
    let kind = FieldKind::Choice { options: &["rectangular", "radial"] };
    assert_eq!(evaluate(&kind, "radial"), Ok(FieldValue::Choice(1)));
    assert_eq!(evaluate(&kind, "Rectangular"), Ok(FieldValue::Choice(0)));
    assert!(evaluate(&kind, "polar").unwrap_err().contains("rectangular, radial"));
    assert_eq!(evaluate(&FieldKind::Check, "true"), Ok(FieldValue::Check(true)));
    assert_eq!(evaluate(&FieldKind::Check, "off"), Ok(FieldValue::Check(false)));
    assert!(evaluate(&FieldKind::Check, "maybe").is_err());
    assert_eq!(evaluate(&FieldKind::Json, r#"{"a": 1}"#), Ok(FieldValue::Json(serde_json::json!({"a": 1}))));
    assert!(evaluate(&FieldKind::Json, "{").is_err());
}

/// A draft edits as the numeric bar's entry: typing replaces a selected
/// text, Backspace pops (or clears a selection), chords do not type.
#[test]
fn text_drafts_edit_as_the_numeric_bar() {
    use bevy::input::keyboard::Key;
    use form::{DraftKey, TextDraft};
    let ch = |s: &str| Key::Character(s.into());
    let mut d = TextDraft { text: "10".into(), select_all: true };
    assert_eq!(d.key(&ch("5"), false), DraftKey::Edited);
    assert_eq!(d, TextDraft { text: "5".into(), select_all: false });
    assert_eq!(d.key(&ch("0"), false), DraftKey::Edited);
    assert_eq!(d.key(&Key::Space, false), DraftKey::Edited);
    assert_eq!(d.text, "50 ");
    assert_eq!(d.key(&Key::Backspace, false), DraftKey::Edited);
    assert_eq!(d.text, "50");
    assert_eq!(d.key(&ch("c"), true), DraftKey::Ignored);
    assert_eq!(d.key(&Key::Space, true), DraftKey::Ignored);
    assert_eq!(d.key(&ch("\u{8}"), false), DraftKey::Ignored);
    assert_eq!(d.key(&Key::Enter, false), DraftKey::Enter);
    assert_eq!(d.key(&Key::Tab, false), DraftKey::Tab);
    assert_eq!(d.key(&Key::Escape, false), DraftKey::Escape);
    assert_eq!(d.text, "50");
    d.select_all = true;
    assert_eq!(d.key(&Key::Backspace, false), DraftKey::Edited);
    assert_eq!(d, TextDraft::default());
    assert_eq!(d.key(&Key::Backspace, false), DraftKey::Ignored);
    // Typing a selected text's own character over it is an edit: the selection clears.
    let mut d = TextDraft { text: "5".into(), select_all: true };
    assert_eq!(d.key(&ch("5"), false), DraftKey::Edited);
    assert_eq!(d, TextDraft { text: "5".into(), select_all: false });
}

/// The pie, palette and form spawn (no duplicate component in a bundle).
#[test]
fn modify_widgets_spawn() {
    #[derive(Component)]
    struct Marker;
    let fonts = UiFonts { regular: default(), italic: default(), mono: default(), icons: Default::default(), medium: default(), semibold: default() };
    let k = Kit::new(&fonts);
    let mut world = World::new();
    let entries = vec![command("cad:mirror", "Mirror", "Modify", &["M"]), command("cad:move", "Move", "Edit", &["m"])];
    let rows = palette::rank(&entries, "");
    let fields = [
        form::FormRow { label: "Count", kind: form::FieldKind::Number { unit: form::Unit::Count, min: Some(1.0), max: Some(10.0), decimals: 0 }, text: "3", focused: true },
        form::FormRow { label: "Spacing", kind: form::FieldKind::Vector { unit: form::Unit::Length }, text: "1, 2", focused: false },
        form::FormRow { label: "Kind", kind: form::FieldKind::Choice { options: &["rectangular", "radial"] }, text: "radial", focused: false },
        form::FormRow { label: "Merge into one body", kind: form::FieldKind::Check, text: "true", focused: false },
        form::FormRow { label: "Components", kind: form::FieldKind::Json, text: "{}", focused: false },
    ];
    let pie = {
        let mut commands = world.commands();
        let pie = k.pie(&mut commands, Vec2::new(400.0, 300.0), "View radial menu", vec![("Top".to_string(), Marker, true), ("Front".to_string(), Marker, false)], Some(0));
        commands.spawn(Node::default()).with_children(|p| {
            k.palette(p, "", &rows, &entries, 0, Marker, |_| Marker);
            k.form(p, "Array", &fields, false, |_| Marker);
        });
        pie
    };
    world.flush();
    assert_eq!(world.get::<Children>(pie).map(|c| c.len()), Some(3), "two entries and the centre dot");
}
