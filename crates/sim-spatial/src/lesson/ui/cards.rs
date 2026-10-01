//! Component and comparison cards.
use super::*;

pub(super) fn component_card(col: &mut ChildSpawnerCommands, k: &Kit, _l: &Learn, b: &sim_lesson::Block, card: &sim_lesson::ComponentCard, builder: Option<&Builder>) {
    let entry = builder.and_then(|b| b.element_entry(&card.component));
    let category = entry.and_then(|e| e.notes.as_ref().map(|n| n.category.clone())).unwrap_or_default();
    let tag = crate::builder::ui::tag_color(crate::builder::ui::category(if category.is_empty() { entry.map(|e| e.domain.as_str()).unwrap_or("") } else { &category }));
    col.spawn((Node { border_radius: BorderRadius::all(Val::Px(7.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), padding: UiRect::all(Val::Px(14.)), border: UiRect::left(Val::Px(3.)), margin: UiRect::vertical(Val::Px(4.)), flex_shrink: 0., ..default() }, BackgroundColor(RAISED), BorderColor::all(tag), BlockNode(b.id.clone()))).with_children(|c| {
        c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(k.text(entry.map(|e| e.display_name.clone()).unwrap_or_else(|| card.component.clone()), 16., TEXT, 2));
            r.spawn(k.mono(&card.component, 11., FAINT));
        });
        let Some(e) = entry else {
            c.spawn(k.text("This component is not in the registry (see sim-lesson check).", 12., WARN, 0));
            return;
        };
        let Some(n) = &e.notes else {
            c.spawn(k.text("No notes for this component yet; its ports and parameters are in the builder library.", 12., SUBTLE, 0));
            return;
        };
        let show: Vec<&str> = if card.show.is_empty() { vec!["summary", "equations", "tradeoffs"] } else { card.show.iter().map(String::as_str).collect() };
        for section in show {
            match section {
                "summary" => {
                    c.spawn(k.text(&n.summary, 13., TEXT, 0));
                }
                "explanation" => paragraph(c, k, "How it works", &n.explanation),
                "equations" => equations(c, k, &n.equations.iter().map(String::as_str).collect::<Vec<_>>()),
                "tradeoffs" => paragraph(c, k, "Trade-offs", &n.tradeoffs),
                "limits" => paragraph(c, k, "Limits", &n.limits),
                "parameters" => {
                    c.spawn(k.section("Parameters"));
                    for p in &e.parameters {
                        c.spawn(k.text(format!("{}{} — {}", p.name, if p.unit.is_empty() { String::new() } else { format!(" ({})", p.unit) }, if p.help.is_empty() { "no description" } else { &p.help }), 12., SUBTLE, 0));
                    }
                }
                _ => {}
            }
        }
    });
}

pub(super) fn compare_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, c: &sim_lesson::Compare) {
    let state = l.compares.get(&c.id);
    col.spawn((card_frame(), BlockNode(b.id.clone()))).with_children(|card| {
        card.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(12.), Val::Px(8.)), ..default() }, BackgroundColor(RAISED))).with_children(|h| {
            h.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.), ..default() }).with_children(|t| {
                t.spawn(k.text(if c.title.is_empty() { format!("Comparison · {}", c.study) } else { c.title.clone() }, 14., TEXT, 2));
                t.spawn(k.text(format!("Saved study `{}` in {} · detailed model", c.study, c.system), 11., SUBTLE, 0));
            });
            let running = state.is_some_and(|s| s.job.is_some());
            if running {
                let (done, total) = state.and_then(|s| s.job.as_ref()?.progress().steps).unwrap_or((0, 0));
                h.spawn(k.text(format!("Running {done}/{total}…"), 12., SUBTLE, 0));
            } else {
                h.spawn(k.button(if state.is_some_and(|s| s.result.is_some()) { "Run again" } else { "Run comparison" }, LessonAction::RunCompare(c.id.clone()), Look::Primary, true));
            }
        });
        card.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.), padding: UiRect::all(Val::Px(12.)), ..default() }).with_children(|body| {
            if !c.caption.is_empty() {
                body.spawn(k.text(&c.caption, 12.5, SUBTLE, 0));
            }
            match state.and_then(|s| s.result.as_ref()) {
                None => {
                    body.spawn(k.text("Runs every variant on the shared runtime (cached once run).", 11.5, FAINT, 0));
                }
                Some(Err(e)) => {
                    body.spawn(k.text(e, 12., WARN, 0));
                }
                Some(Ok(run)) => {
                    let table = compare_table(run);
                    crate::markdown::table(body, &page_theme(k), &table);
                    if run.variants.iter().any(|v| v.2.is_some()) {
                        body.spawn(k.text("A variant that failed shows its error instead of values.", 11., FAINT, 0));
                    }
                }
            }
        });
    });
}

/// One row per variant, one column per metric; numbers right-aligned.
fn compare_table(run: &CompareRun) -> sim_markdown::Table {
    use sim_markdown::Align;
    let mut metrics: Vec<String> = Vec::new();
    for (_, values, _) in &run.variants {
        for (label, _) in values {
            if !metrics.contains(label) {
                metrics.push(label.clone());
            }
        }
    }
    let head: Vec<String> = std::iter::once("Variant".to_string()).chain(metrics.iter().cloned()).collect();
    let rows: Vec<Vec<String>> = run
        .variants
        .iter()
        .map(|(label, values, error)| {
            let mut row = vec![label.clone()];
            match error {
                Some(e) => row.push(format!("failed: {e}")),
                None => row.extend(metrics.iter().map(|m| values.iter().find(|(l, _)| l == m).map(|(_, v)| if v.is_finite() { num(*v) } else { "–".into() }).unwrap_or_else(|| "–".into()))),
            }
            row
        })
        .collect();
    let align: Vec<Align> = std::iter::once(Align::Left).chain(metrics.iter().map(|_| Align::Right)).collect();
    sim_markdown::Table::plain(&head, &rows, &align)
}
