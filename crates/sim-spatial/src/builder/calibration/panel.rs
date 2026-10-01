//! The Measured evidence section of the Actuators tab: archive metadata and
//! integrity, split × outcome counts, filters, the selected trial and the
//! paged trial list.
use super::*;

/// Encoder counts for a value in radians (display alongside the archive's rad).
fn counts_of(rad: f64) -> String {
    format!("{:.1}", rad / hx_archive::ENCODER_QUANTUM_RAD)
}

const HELD_OUT: Color = Color::srgb(0.690, 0.604, 0.902);

/// The Measured evidence section: path, Reload/Cancel, archive metadata and
/// integrity, split × outcome counts, filters and a paged trial list.
pub(in crate::builder) fn section(body: &mut ChildSpawnerCommands, k: &Kit, b: &Builder) {
    let c = &b.calibration;
    body.spawn(k.caption("Measured identification evidence as archived: each hardware trial's measured encoder trace against the fitted model's prediction, with the archive's own pass/fail. Read-only; nothing is evaluated, refitted or promoted here."));
    body.spawn(k.section("Archive"));
    let focused = b.input.as_ref().is_some_and(|i| i.purpose == Purpose::CalibrationArchive);
    let shown = if focused { b.input.as_ref().map(|i| i.buffer.clone()).unwrap_or_default() } else { c.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default() };
    body.spawn(k.input(&shown, "Path to an identification archive folder · Enter to load", BuildAction::CalibrationPath, focused));
    body.spawn(Node { margin: UiRect::top(Val::Px(6.)), ..wrap() }).with_children(|r| {
        r.spawn(k.button("Reload", BuildAction::CalibrationReload, Look::Secondary, c.pending().is_none()));
        if c.pending().is_some() {
            r.spawn(k.button("Cancel", BuildAction::CancelCalibration, Look::Danger, true));
        }
    });
    if let Some(pending) = c.pending() {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(ACCENT), k.text(format!("Loading {}…", pending.display()), size::SMALL, TEXT, 0)]));
    }
    if let Some(e) = &c.error {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, margin: UiRect::top(Val::Px(6.)), flex_shrink: 0., ..default() }, children![k.dot(DANGER), k.text(e, size::SMALL, DANGER, 0)]));
    }
    let Some(r) = &c.shown else {
        if c.pending().is_none() && c.error.is_none() {
            body.spawn(k.caption("Not loaded yet."));
        }
        return;
    };
    let a = &r.archive;
    if c.error.is_some() {
        body.spawn(k.text(format!("Still showing the last good load: {}", r.path.display()), size::CAPTION, WARN, 1));
    }
    body.spawn(k.text(format!("Showing {}", r.path.display()), size::DETAIL, FAINT, 0));
    selected_block(body, k, c);
    k.property(body, "Label", &a.label, "", None::<BuildAction>, false);
    body.spawn(k.text("Interpretation", size::DETAIL, FAINT, 2));
    body.spawn(k.text(&a.interpretation, size::CAPTION, TEXT, 0));
    body.spawn(k.text("Split policy", size::DETAIL, FAINT, 2));
    body.spawn(k.text(&a.split_policy, size::CAPTION, TEXT, 0));

    body.spawn(k.section("Integrity"));
    body.spawn(k.text("observations.json blake3", size::DETAIL, FAINT, 2));
    body.spawn(k.text(&a.observation_blake3, 10.5, TEXT, 0));
    body.spawn(k.text("results.json (model) blake3", size::DETAIL, FAINT, 2));
    body.spawn(k.text(&a.model_blake3, 10.5, TEXT, 0));
    let all_verified = a.verified_inputs == a.input_blake3.len();
    k.property(body, "Verified inputs", &format!("{} of {}", a.verified_inputs, a.input_blake3.len()), "", None::<BuildAction>, false);
    body.spawn(k.text(format!("Inputs hashed against {}", r.repository.display()), 10.5, FAINT, 0));
    if a.integrity_issues.is_empty() {
        body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(if all_verified { OK } else { WARN }), k.text("Integrity issues: none", size::CAPTION, TEXT, 0)]));
    } else {
        for issue in &a.integrity_issues {
            body.spawn((Node { column_gap: Val::Px(8.), align_items: AlignItems::Start, flex_shrink: 0., ..default() }, children![k.dot(WARN), k.text(issue, size::CAPTION, WARN, 0)]));
        }
    }

    let n = counts(a);
    body.spawn(k.section(&format!("Trials  {}", a.trials.len())));
    body.spawn(k.text("Counts of the archive's own comparison.passes:", size::DETAIL, FAINT, 0));
    let line = |label: &str, x: Count| format!("{label}: {}/{} pass · {} fail", x.pass, x.total, x.fail);
    body.spawn(k.text(line("Held-out (validation)", n.held_out), size::SMALL, HELD_OUT, 2));
    body.spawn(k.text(line("Train (fitting)", n.train), size::SMALL, TEXT, 1));
    for (split, x) in &n.by_split {
        body.spawn(k.text(line(&format!("  {split}"), *x), size::DETAIL, SUBTLE, 0));
    }
    if let Some(t) = a.trials.first() {
        body.spawn(k.text(format!("Limits per trial: RMSE ≤ {} {u} ({} counts) and |final error| ≤ {} {u} ({} counts); 1 count = 2π/4096 rad.", num(t.limits.rmse), counts_of(t.limits.rmse), num(t.limits.final_abs_error), counts_of(t.limits.final_abs_error), u = t.measured.unit), size::DETAIL, FAINT, 0));
    }

    body.spawn(Node { margin: UiRect::top(Val::Px(6.)), ..wrap() }).with_children(|chips| {
        for (label, f) in [("All splits", SplitFilter::All), ("Train", SplitFilter::Train), ("Held-out", SplitFilter::HeldOut)] {
            chips.spawn(k.chip(label, BuildAction::CalibrationSplit(f), c.split == f, true));
        }
    });
    body.spawn(Node { margin: UiRect::top(Val::Px(4.)), ..wrap() }).with_children(|chips| {
        for (label, f) in [("All outcomes", OutcomeFilter::All), ("Pass", OutcomeFilter::Pass), ("Fail", OutcomeFilter::Fail)] {
            chips.spawn(k.chip(label, BuildAction::CalibrationOutcome(f), c.outcome == f, true));
        }
    });
    let visible = c.visible();
    let pages = c.pages(visible.len());
    let page = c.page.min(pages - 1);
    body.spawn(k.text(format!("{} trials match · page {} of {pages}", visible.len(), page + 1), size::DETAIL, FAINT, 0));
    body.spawn(Node { margin: UiRect::top(Val::Px(4.)), ..wrap() }).with_children(|r| {
        r.spawn(k.button("‹ Previous", BuildAction::CalibrationPage(page.saturating_sub(1)), Look::Secondary, page > 0));
        r.spawn(k.button("Next ›", BuildAction::CalibrationPage(page + 1), Look::Secondary, page + 1 < pages));
    });
    for &i in visible.iter().skip(page * PAGE_ROWS).take(PAGE_ROWS) {
        trial_row(body, k, &a.trials[i], c.selected.as_deref() == Some(a.trials[i].id.as_str()));
    }
}

/// The selected trial: split role, archive metrics against limits, and the
/// chart of measured against predicted (shared raster), above the metadata so
/// it is visible without scrolling.
fn selected_block(body: &mut ChildSpawnerCommands, k: &Kit, c: &CalibrationState) {
    let Some(t) = c.selected_trial() else {
        body.spawn(k.text("Select a trial below to chart its measured trace against the model's prediction.", size::CAPTION, SUBTLE, 0));
        return;
    };
    let held = held_out(t);
    let x = &t.comparison;
    let u = &t.measured.unit;
    body.spawn(k.section(&format!("Trial  {}", t.id)));
    let role = if held { "HELD-OUT (validation data)" } else { "TRAIN (fitting data)" };
    body.spawn(k.text(format!("{role} · split {}", t.split), size::ITEM, if held { HELD_OUT } else { TEXT }, 2));
    body.spawn(k.text(format!("run {} · device {} · stage {} · {} · drive {:+.1}% · {} ms", t.run, t.device, t.stage, t.kind, t.drive * 100., num(t.duration_s * 1000.)), 10.5, SUBTLE, 0));
    let (verdict, color) = if x.passes { ("✓ pass (archive comparison)", OK) } else { ("× fail (archive comparison)", DANGER) };
    body.spawn(k.text(verdict, size::SMALL, color, 2));
    let within = |v: f64, limit: f64| if v.abs() <= limit { "within" } else { "over" };
    body.spawn(k.text(format!("RMSE {} {u} ({} counts) · limit {} {u} · {}", num(x.rmse), counts_of(x.rmse), num(t.limits.rmse), within(x.rmse, t.limits.rmse)), size::DETAIL, TEXT, 0));
    body.spawn(k.text(format!("final error {} {u} ({} counts) · limit |·| {} {u} · {}", num(x.final_error), counts_of(x.final_error), num(t.limits.final_abs_error), within(x.final_error, t.limits.final_abs_error)), size::DETAIL, TEXT, 0));
    body.spawn(k.text(format!("max |error| {} {u} ({} counts)", num(x.maximum_abs_error), counts_of(x.maximum_abs_error)), size::DETAIL, SUBTLE, 0));
    body.spawn(k.text(format!("{} [{u}] against time [s]", t.measured.quantity.name), size::DETAIL, FAINT, 2));
    for (trace, label, [r, g, bl]) in [(&t.measured, MEASURED_LABEL, SERIES[0]), (&t.predicted, PREDICTED_LABEL, SERIES[1])] {
        let color = Color::srgb_u8(r, g, bl);
        let line = if trace.samples.is_empty() { format!("{label}: not in archive") } else { format!("{label} · {} samples", trace.samples.len()) };
        body.spawn((Node { column_gap: Val::Px(6.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, children![k.dot(color), k.text(line, size::DETAIL, if trace.samples.is_empty() { WARN } else { color }, 1)]));
    }
    let Some(image) = &c.chart else { return };
    let ((lo, hi), (t0, t1)) = c.axes;
    body.spawn(k.chart_image(image.clone(), Node { width: Val::Percent(100.), height: Val::Px(150.), margin: UiRect::top(Val::Px(4.)), flex_shrink: 0., ..default() }, true))
        .with_children(|plot| {
            if t.measured.samples.is_empty() && t.predicted.samples.is_empty() {
                // A message across the empty chart (not an axis label).
                plot.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(8.), top: Val::Px(60.), ..default() }, children![k.text("No trace in the archive for this trial", size::DETAIL, WARN, 1)]));
                return;
            }
            plot.spawn(k.chart_label(format!("{} {u}", num(hi)), Corner::TopLeft));
            plot.spawn(k.chart_label(format!("{} {u}", num(lo)), Corner::BottomLeft));
            plot.spawn(k.chart_label(format!("{:.3} – {:.3} s", t0, t1), Corner::BottomRight));
        });
}

fn trial_row(body: &mut ChildSpawnerCommands, k: &Kit, t: &Trial, selected: bool) {
    let held = held_out(t);
    let x = &t.comparison;
    let u = &t.measured.unit;
    let (verdict, color) = if x.passes { ("✓ pass", OK) } else { ("× fail", DANGER) };
    body.spawn((
        Button,
        BuildAction::CalibrationTrial(t.id.clone()),
        bevy::ui::prelude::AccessibleLabel::new(format!("Trial {}", t.id)),
        Tint::selectable(selected),
        Node { border_radius: BorderRadius::all(Val::Px(4.)), flex_direction: FlexDirection::Column, padding: UiRect::axes(Val::Px(8.), Val::Px(3.)), margin: UiRect::top(Val::Px(3.)), border: UiRect::left(Val::Px(if selected { 3. } else { 2. })), flex_shrink: 0., ..default() },
        BorderColor::all(if selected { ACCENT } else if held { HELD_OUT } else { Color::NONE }),
        BackgroundColor(if selected { ACCENT_BG } else { Color::NONE }),
    ))
    .with_children(|row| {
        row.spawn(Node { justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(8.), ..default() }).with_children(|top| {
            top.spawn(k.text(&t.id, size::CAPTION, TEXT, 1));
            top.spawn(k.text(verdict, size::CAPTION, color, 2));
        });
        row.spawn(k.text(format!("{} · {}", if held { "held-out (validation)" } else { "train (fitting)" }, t.split), 10.5, if held { HELD_OUT } else { SUBTLE }, 1));
        row.spawn(k.text(format!("device {} · stage {} · {} · drive {:+.1}% · {} ms", t.device, t.stage, t.kind, t.drive * 100., num(t.duration_s * 1000.)), 10.5, SUBTLE, 0));
        row.spawn(k.text(format!("{}–{} V · {}–{} °C", num(t.voltage_range_v[0]), num(t.voltage_range_v[1]), num(t.temperature_range_c[0]), num(t.temperature_range_c[1])), 10.5, SUBTLE, 0));
        row.spawn(k.text(format!("RMSE {} {u} ({} counts), limit {} {u} · final error {} {u} ({} counts), limit |·| {} {u}", num(x.rmse), counts_of(x.rmse), num(t.limits.rmse), num(x.final_error), counts_of(x.final_error), num(t.limits.final_abs_error)), 10.5, color, 0));
    });
}
