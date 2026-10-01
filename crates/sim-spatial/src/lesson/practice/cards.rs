//! Question and self-explanation cards, the locked-rest marker, the
//! review list and figures.
use super::*;
use super::sketch::sketch_canvas;

fn badge(k: &Kit, text: &str, color: Color) -> impl Bundle {
    (Node { border_radius: BorderRadius::all(Val::Px(10.)), padding: UiRect::axes(Val::Px(8.), Val::Px(2.)), border: UiRect::all(Val::Px(1.)), ..default() }, BorderColor::all(color), children![k.text(text, 10.5, color, 2)])
}

/// A question card.
pub(in crate::lesson) fn quiz_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, q: &Quiz, theme: &crate::markdown::Theme) {
    let slug = l.slug().unwrap_or_default();
    let record = l.progress.quiz(slug, &q.id);
    let reviewing = l.review.as_ref().is_some_and(|(s, id)| s == slug && *id == q.id);
    let verdict = l.quiz_verdict.get(&q.id);
    let predicted = record.and_then(|r| r.prediction.clone());
    // A pretest is done once guessed, right or wrong.
    let passed = record.is_some_and(|r| r.passed() || (q.pretest && r.attempts > 0)) && !reviewing;
    let wrong_tries = record.map(|r| if r.correct { 0 } else { r.attempts }).unwrap_or(0);
    let (label, color) = match (q.kind, reviewing) {
        (_, true) => ("Review · from memory", ACCENT),
        (kind, _) if kind.predicts() => ("Predict first", Color::srgb(0.66, 0.55, 0.93)),
        _ if q.pretest => ("Before you read on: a guess", Color::srgb(0.66, 0.55, 0.93)),
        (QuizKind::Steps, _) => ("Finish the worked example", ACCENT),
        _ => ("Check your understanding", ACCENT),
    };
    // This attempt's numbers: varied values and the model's `given` values.
    let mut values = record.map(|r| r.values.clone()).unwrap_or_default();
    values.extend(l.model.given.get(&q.id).cloned().unwrap_or_default());
    col.spawn((Node { border_radius: BorderRadius::all(Val::Px(9.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.), padding: UiRect::all(Val::Px(16.)), border: UiRect::all(Val::Px(1.)), margin: UiRect::vertical(Val::Px(8.)), flex_shrink: 0., ..default() }, BackgroundColor(RAISED), BorderColor::all(if passed { OK.with_alpha(0.6) } else { color.with_alpha(0.6) }), super::super::ui::BlockNode(b.id.clone()))).with_children(|c| {
        c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(badge(k, label, color));
            let state = if passed {
                if record.is_some_and(|r| r.revealed && !r.correct) { "answer shown · review tomorrow".to_string() } else { "answered".to_string() }
            } else if let Some(p) = &predicted {
                // A sketch's prediction is its curve, drawn below, not text.
                if q.kind == QuizKind::Sketch { "your curve is locked in".to_string() } else { format!("your prediction: {p}") }
            } else if wrong_tries > 0 {
                format!("{wrong_tries} tr{} so far", if wrong_tries == 1 { "y" } else { "ies" })
            } else if q.pretest {
                "a guess is fine".into()
            } else if q.gates() && !reviewing {
                "answer to continue".into()
            } else {
                String::new()
            };
            r.spawn(k.text(state, 11., if passed { OK } else { SUBTLE }, 1));
        });
        crate::markdown::render(c, &sim_markdown::parse(&Quiz::fill(&q.question, &values)), theme, |_| None::<LessonAction>);
        if !q.vary.is_empty() {
            c.spawn(k.text("The numbers change each time this question comes back.", 10.5, FAINT, 0));
        }
        let chosen = l.quiz_pick.get(&q.id).copied();
        let show_answer = passed && !reviewing;
        if !q.options.is_empty() {
            for (i, o) in q.options.iter().enumerate() {
                let picked = chosen == Some(i);
                let judged = verdict.is_some() && picked;
                let border = if show_answer && o.correct { OK } else if judged { if o.correct { OK } else { WARN } } else if picked { ACCENT } else { BORDER };
                let tint = if picked { Tint::new(HOVER_BG, HOVER_BG) } else { Tint::SURFACE };
                c.spawn((Button, LessonAction::QuizPick(q.id.clone(), i), tint, AccessibleLabel::new(format!("{}: {}", (b'A' + i as u8) as char, o.text)), Node { border_radius: BorderRadius::all(Val::Px(6.)), padding: UiRect::axes(Val::Px(12.), Val::Px(9.)), border: UiRect::all(Val::Px(1.5)), column_gap: Val::Px(10.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, BackgroundColor(tint.idle), BorderColor::all(border))).with_children(|row| {
                    row.spawn(k.text(format!("{}", (b'A' + i as u8) as char), 12., if picked { ACCENT } else { FAINT }, 2));
                    row.spawn((k.text(&o.text, 13.5, TEXT, 0), Node { flex_shrink: 1., min_width: Val::Px(0.), ..default() }));
                });
            }
        } else if q.kind == QuizKind::Sketch {
            sketch_canvas(c, k, l, q, predicted.is_some());
        } else if q.kind == QuizKind::Steps {
            // The worked example: done steps shown, blanks to fill in order.
            for (i, st) in q.steps.iter().enumerate() {
                c.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.), padding: UiRect::axes(Val::Px(10.), Val::Px(6.)), border: UiRect::left(Val::Px(2.)), flex_shrink: 0., ..default() }).insert(BorderColor::all(if st.blank() { ACCENT } else { BORDER })).with_children(|row| {
                    row.spawn(k.text(format!("Step {} · {}", i + 1, st.prompt), 12.5, SUBTLE, 1));
                    match &st.worked {
                        Some(w) => {
                            crate::markdown::render(row, &sim_markdown::parse(w), theme, |_| None::<LessonAction>);
                        }
                        None => {
                            let focused = l.input.as_ref().is_some_and(|x| x.purpose == Purpose::StepAnswer(q.id.clone(), i));
                            let shown = if focused { l.input.as_ref().map(|x| x.buffer.clone()).unwrap_or_default() } else { l.step_text.get(&(q.id.clone(), i)).cloned().unwrap_or_default() };
                            row.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|r| {
                                r.spawn(Node { width: Val::Px(180.), flex_direction: FlexDirection::Column, ..default() }).with_children(|w| {
                                    w.spawn(k.input(&shown, "Your number", LessonAction::StepInput(q.id.clone(), i), focused));
                                });
                                if !st.unit.is_empty() {
                                    r.spawn(k.text(&st.unit, 13., SUBTLE, 0));
                                }
                            });
                        }
                    }
                });
            }
        } else {
            let focused = l.input.as_ref().is_some_and(|i| i.purpose == Purpose::QuizAnswer(q.id.clone()));
            let shown = if focused { l.input.as_ref().map(|i| i.buffer.clone()).unwrap_or_default() } else { l.quiz_text.get(&q.id).cloned().or(predicted.clone()).unwrap_or_default() };
            c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|r| {
                r.spawn(Node { width: Val::Px(180.), flex_direction: FlexDirection::Column, ..default() }).with_children(|w| {
                    w.spawn(k.input(&shown, "Type a number", LessonAction::QuizInput(q.id.clone()), focused));
                });
                if !q.unit.is_empty() {
                    r.spawn(k.text(&q.unit, 13., SUBTLE, 0));
                }
            });
        }
        // How sure: asked before checking (graded questions only).
        let gradable = !q.kind.predicts() && !q.pretest && !show_answer;
        if gradable {
            c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.), ..default() }).with_children(|r| {
                r.spawn(k.text("How sure are you?", 11.5, SUBTLE, 0));
                use sim_lesson::progress::Confidence;
                for (level, name, c) in [(0u8, "A guess", Confidence::Guess), (1, "Fairly sure", Confidence::FairlySure), (2, "Sure", Confidence::Sure)] {
                    r.spawn(k.button(name, LessonAction::Confidence(q.id.clone(), level), Look::Chip(l.confidence.get(&q.id) == Some(&c)), true));
                }
            });
        }
        // Actions.
        c.spawn(wrap()).with_children(|r| {
            let committed = q.kind.predicts() && predicted.is_some();
            if q.kind == QuizKind::Sketch && !committed {
                r.spawn(k.button("Clear", LessonAction::SketchClear(q.id.clone()), Look::Ghost, l.sketches.contains_key(&q.id)));
            }
            if !show_answer && !committed {
                let label = match q.kind {
                    QuizKind::Predict => "Lock in my prediction",
                    QuizKind::Sketch => "Lock in my sketch",
                    _ => "Check",
                };
                r.spawn(k.button(label, LessonAction::QuizCheck(q.id.clone()), Look::Primary, true));
            }
            // "Show me": jump the scene to the moment this question is about.
            if let Some(t) = q.moment {
                if let Some(scene) = q.scene.clone().or_else(|| l.lesson.as_ref().and_then(|x| x.scenes().next().map(|(_, s)| s.id.clone()))) {
                    r.spawn(k.button("Show me in the scene", LessonAction::ShowAt { scene, time: Some(t), part: None }, Look::Ghost, true));
                }
            }
            // The hint ladder, one rung at a time.
            let ladder = q.hint_ladder();
            let used = record.map(|r| r.hints_used as usize).unwrap_or(0).min(ladder.len());
            if used < ladder.len() && !show_answer && !q.kind.predicts() && !q.pretest {
                r.spawn(k.button(&format!("Show a hint ({} of {})", used + 1, ladder.len()), LessonAction::HintMore(q.id.clone()), Look::Ghost, true));
            }
            if wrong_tries >= 2 && !show_answer && !q.kind.predicts() {
                r.spawn(k.button("Show the answer", LessonAction::QuizReveal(q.id.clone()), Look::Ghost, true));
            }
            // In a review session: on to the next due question.
            if let (true, Some((queue, at))) = (reviewing || l.quiz_verdict.get(&q.id).is_some_and(|v| v.correct), l.session.as_ref()) {
                if queue.get(*at).is_some_and(|(ls, qid)| ls == slug && qid == &q.id) {
                    let last = at + 1 >= queue.len();
                    r.spawn(k.button(if last { "Finish the review" } else { "Next review question" }, LessonAction::ReviewNext, Look::Primary, l.quiz_verdict.get(&q.id).is_some_and(|v| v.correct)));
                    r.spawn(k.text(format!("{} of {}", at + 1, queue.len()), 11.5, FAINT, 0));
                }
            }
        });
        let ladder = q.hint_ladder();
        let used = record.map(|r| r.hints_used as usize).unwrap_or(0).min(ladder.len());
        for (i, h) in ladder[..used].iter().enumerate() {
            c.spawn(k.text(format!("Hint {}: {h}", i + 1), 12.5, SUBTLE, 0));
        }
        // Feedback: this answer's, then the hint after a miss, then the worked explanation once answered.
        if let Some(v) = verdict {
            c.spawn(Node { column_gap: Val::Px(8.), align_items: AlignItems::FlexStart, ..default() }).with_children(|r| {
                // A prediction is only committed here; the run judges it below.
                r.spawn((k.text(if q.pretest { "Noted." } else if q.kind.predicts() && v.correct { "Locked in." } else if v.correct { "Right." } else { "Not yet." }, 13., if v.correct || q.pretest { OK } else { WARN }, 2), Node { flex_shrink: 0., ..default() }));
                r.spawn((k.text(if v.feedback.is_empty() { if v.correct { "Correct." } else { "Not quite — try again." } } else { v.feedback.as_str() }, 13., TEXT, 0), Node { flex_shrink: 1., min_width: Val::Px(0.), ..default() }));
            });
            if let (false, Some(r)) = (v.correct, l.remedy.get(&q.id)) {
                super::super::extras::remedy_card(c, k, l, r, theme);
            }
            if q.pretest && !q.explain.is_empty() {
                crate::markdown::render(c, &sim_markdown::parse(&q.explain), theme, |_| None::<LessonAction>);
            }
        }
        // Sketches: drawn over the simulated curve once the scene has played past the window.
        if q.kind == QuizKind::Sketch {
            let played = l.scene.as_ref().filter(|a| Some(&a.id) == q.scene.as_ref()).is_some_and(|a| a.run.is_some() && a.time >= l.sketch_window(q)[1].min(a.duration()) - 1e-6);
            match (predicted.is_some(), l.sketch_results.get(&q.id).filter(|_| played)) {
                (true, Some((image, gap, _))) => {
                    c.spawn(k.chart_image(image.clone(), Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Percent(100.), aspect_ratio: Some(720. / 200.), flex_shrink: 0., ..default() }, false));
                    let close = *gap <= q.tolerance.map(|t| match t { sim_lesson::quiz::Tolerance::Absolute(v) | sim_lesson::quiz::Tolerance::Relative(v) => v }).unwrap_or(0.15);
                    c.spawn(k.text(format!("Purple: your sketch. Teal: the simulation. {} (typical gap {:.0} % of the axis).", if close { "Your curve has the right shape and size" } else { "Your curve differs from the simulation" }, gap * 100.), 13., if close { OK } else { WARN }, 1));
                    if !q.explain.is_empty() {
                        crate::markdown::render(c, &sim_markdown::parse(&q.explain), theme, |_| None::<LessonAction>);
                    }
                }
                (true, None) => {
                    c.spawn(k.text("Now play the scene and watch how the real curve compares with yours.", 12.5, SUBTLE, 0));
                }
                _ => {
                    c.spawn(k.text("Drag across the chart to draw the curve you expect. Sketching first makes the real result stick, even when the sketch is off.", 11.5, FAINT, 0));
                }
            }
        }
        // Predictions are judged against the recorded run once it has played.
        if q.kind == QuizKind::Predict {
            if let (Some(p), Some(a)) = (&predicted, l.scene.as_ref().filter(|a| Some(&a.id) == q.scene.as_ref())) {
                if let Some(run) = a.run.as_ref().filter(|_| a.time >= q.window.map(|w| w[1]).unwrap_or(a.duration()).min(a.duration()) - 1e-6) {
                    let verdict = match (sim_lesson::units::read(p, &q.unit).ok().map(|r| r.value), q.options.is_empty()) {
                        (Some(v), true) => runtime::predicted_value(run, q).map(|sim| q.check(&Answer::Number(v), Some(sim))),
                        _ => q.options.iter().position(|o| &o.text == p).map(|i| q.check(&Answer::Choice(i), None)),
                    };
                    if let Some(Ok(v)) = verdict {
                        let sim = runtime::predicted_value(run, q).map(|x| format!("The simulation gave {x:.4} {}. ", q.unit)).unwrap_or_default();
                        c.spawn(k.text(format!("{}{}{}", sim, if v.correct { "Your prediction was right. " } else { "Your prediction was off: " }, v.feedback), 13., if v.correct { OK } else { WARN }, 1));
                        if !q.explain.is_empty() {
                            crate::markdown::render(c, &sim_markdown::parse(&q.explain), theme, |_| None::<LessonAction>);
                        }
                    }
                } else {
                    c.spawn(k.text("Now play the scene to the end and see how close you were.", 12.5, SUBTLE, 0));
                }
            } else if predicted.is_none() {
                c.spawn(k.text("Commit to a guess before you watch: predicting first makes the result stick, even when the guess is wrong.", 11.5, FAINT, 0));
            }
        } else if show_answer {
            if let Some(o) = q.options.iter().find(|o| o.correct).filter(|_| record.is_some_and(|r| r.revealed && !r.correct)) {
                c.spawn(k.text(format!("Answer: {}", o.text), 13., OK, 1));
            }
            if let Some(a) = q.answer.filter(|_| record.is_some_and(|r| r.revealed && !r.correct)) {
                c.spawn(k.text(format!("Answer: {a} {}", q.unit), 13., OK, 1));
            }
            if !q.explain.is_empty() {
                c.spawn(k.section("Why"));
                crate::markdown::render(c, &sim_markdown::parse(&q.explain), theme, |_| None::<LessonAction>);
            }
        }
    });
}

/// A self-explanation prompt.
pub(in crate::lesson) fn reflect_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, r: &Reflect, theme: &crate::markdown::Theme) {
    let slug = l.slug().unwrap_or_default();
    let saved = l.progress.lessons.get(slug).and_then(|p| p.reflections.get(&r.id));
    let focused = l.input.as_ref().is_some_and(|i| i.purpose == Purpose::Reflection(r.id.clone()));
    col.spawn((Node { border_radius: BorderRadius::all(Val::Px(7.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.), padding: UiRect::all(Val::Px(16.)), border: UiRect::left(Val::Px(3.)), margin: UiRect::vertical(Val::Px(8.)), flex_shrink: 0., ..default() }, BackgroundColor(RAISED), BorderColor::all(Color::srgb(0.93, 0.76, 0.40)), super::super::ui::BlockNode(b.id.clone()))).with_children(|c| {
        let recall = r.kind == sim_lesson::quiz::ReflectKind::Recall;
        c.spawn(badge(k, if recall { "Recall first" } else { "Explain it in your own words" }, Color::srgb(0.93, 0.76, 0.40)));
        crate::markdown::render(c, &sim_markdown::parse(&r.prompt), theme, |_| None::<LessonAction>);
        let text = if focused { l.input.as_ref().map(|i| i.buffer.clone()).unwrap_or_default() } else { saved.map(|s| s.text.clone()).unwrap_or_default() };
        c.spawn((Button, LessonAction::ReflectInput(r.id.clone()), Tint::SURFACE, AccessibleLabel::new(if text.is_empty() { "Write your explanation" } else { text.as_str() }), Node { border_radius: BorderRadius::all(Val::Px(6.)), min_height: Val::Px(70.), padding: UiRect::all(Val::Px(10.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() }, BackgroundColor(SURFACE), BorderColor::all(if focused { ACCENT } else { BORDER }))).with_children(|f| {
            f.spawn(k.text(if focused { format!("{text}|") } else if text.is_empty() { "Click and write two or three sentences…".into() } else { text.clone() }, 13.5, if text.is_empty() && !focused { FAINT } else { TEXT }, 0));
        });
        c.spawn(wrap()).with_children(|row| {
            if focused {
                row.spawn(k.button("Save", LessonAction::ReflectSave(r.id.clone()), Look::Primary, true));
                row.spawn(k.button("Cancel", LessonAction::CancelDraft, Look::Ghost, true));
                row.spawn(k.text("Cmd/Ctrl+Enter saves · kept on this machine only", 10.5, FAINT, 0));
            } else if saved.is_some() {
                row.spawn(k.text("Saved", 11.5, OK, 1));
            }
        });
        // After writing: which key ideas the text covers (cue words, instant, on this machine).
        if let (Some(saved), false, false) = (saved, focused, r.key_points.is_empty()) {
            let coverage = sim_lesson::quiz::coverage(&saved.text, &r.key_points);
            if recall {
                c.spawn(k.section("Tick what you remembered"));
                let ticked = l.progress.lessons.get(slug).and_then(|p| p.recalled.get(&r.id)).cloned().unwrap_or_default();
                for (i, (idea, seen)) in coverage.iter().enumerate() {
                    let on = ticked.contains(idea);
                    c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|row| {
                        row.spawn(k.button(&format!("{} {idea}", if on { "[x]" } else { "[ ]" }), LessonAction::RecallTick(r.id.clone(), i), Look::Chip(on), true));
                        if *seen && !on {
                            row.spawn(k.text("your text mentions it", 10.5, FAINT, 0));
                        }
                    });
                }
                c.spawn(k.text("Anything you did not tick is worth rereading before you go on.", 11., FAINT, 0));
            } else {
                c.spawn(k.section("What your explanation covers"));
                for (idea, seen) in &coverage {
                    c.spawn(k.text(format!("{} {idea}", if *seen { "covered:" } else { "not yet:" }), 12., if *seen { OK } else { SUBTLE }, 0));
                }
                c.spawn(k.text("A quick word check, not a grade: it only looks for the key ideas' words.", 10.5, FAINT, 0));
            }
        }
        if saved.is_some() && !focused && !recall {
            c.spawn(wrap()).with_children(|row| {
                row.spawn(k.button("Ask Codex for feedback", LessonAction::ReflectFeedback(r.id.clone()), Look::Ghost, true));
                row.spawn(k.text("posted as a note on this prompt; the reply appears there", 10.5, FAINT, 0));
            });
        }
        if saved.is_some() && !r.model_answer.is_empty() && !focused {
            c.spawn(k.section("Compare with an expert's explanation"));
            crate::markdown::render(c, &sim_markdown::parse(&r.model_answer), theme, |_| None::<LessonAction>);
        }
    });
}

/// What stands in for the locked rest of the lesson.
pub(in crate::lesson) fn locked(col: &mut ChildSpawnerCommands, k: &Kit, hidden: usize) {
    col.spawn((Node { border_radius: BorderRadius::all(Val::Px(9.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), padding: UiRect::all(Val::Px(18.)), border: UiRect::all(Val::Px(1.)), align_items: AlignItems::Center, margin: UiRect::vertical(Val::Px(10.)), flex_shrink: 0., ..default() }, BorderColor::all(BORDER))).with_children(|c| {
        c.spawn(k.text("The next part unlocks when you answer the question above", 14., TEXT, 2));
        c.spawn(k.text(format!("{hidden} more block{} below. Take your time: working it out is what makes it stick.", if hidden == 1 { "" } else { "s" }), 12., SUBTLE, 0));
    });
}

/// Review list in the outline (questions due across lessons).
pub(in crate::lesson) fn review_outline(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn) {
    let due = l.progress.due(sim_lesson::progress::now());
    if due.is_empty() {
        return;
    }
    col.spawn(k.section(&format!("Review · {} due", due.len())));
    col.spawn(k.text("Spaced retrieval: answer from memory; each success pushes the next review further out.", 10.5, FAINT, 0));
    if due.len() > 1 && l.session.is_none() {
        col.spawn(k.button(&format!("Start a mixed review ({})", due.len()), LessonAction::ReviewSession, Look::Secondary, true));
        col.spawn(k.text("Questions from different lessons, interleaved: harder, and it sticks better.", 10.5, FAINT, 0));
    }
    if let Some((queue, at)) = &l.session {
        col.spawn(k.text(format!("Mixed review: {} of {}", at + 1, queue.len()), 11.5, ACCENT, 1));
        col.spawn(k.button("End the review", LessonAction::ReviewEnd, Look::Ghost, true));
    }
    for (lesson, quiz) in due.iter().take(6) {
        let title = l.entries.iter().find(|e| &e.slug == lesson).map(|e| e.title.clone()).unwrap_or_else(|| lesson.clone());
        col.spawn(k.button(&format!("{title} · {quiz}"), LessonAction::Review(lesson.clone(), quiz.clone()), Look::Ghost, true));
    }
}

/// A figure: the image at its natural aspect ratio, and its caption.
pub(in crate::lesson) fn figure(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, src: &str, caption: &str) {
    let Some(lesson) = &l.lesson else { return };
    let path = lesson.figure_path(src);
    let id = std::path::Path::new(src).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    col.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), align_items: AlignItems::Center, margin: UiRect::vertical(Val::Px(8.)), width: Val::Percent(100.), flex_shrink: 0., ..default() }, super::super::ui::BlockNode(format!("figure:{id}")))).with_children(|c| {
        match l.figures.get(&path).map(|f| &f.state) {
            Some(FigureState::Ready { image, aspect, units }) => {
                c.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), width: Val::Percent(100.), max_width: Val::Px(760.), aspect_ratio: Some(*aspect), ..default() }, ImageNode::new(image.clone()), super::super::narrate::FigureNode(id.clone(), units.0, units.1)));
            }
            Some(FigureState::Failed(e)) => {
                c.spawn(k.text(format!("Figure {src}: {e}"), 12., WARN, 0));
            }
            _ => {
                c.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), width: Val::Percent(100.), height: Val::Px(160.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(SURFACE), children![k.text("Drawing figure…", 12., FAINT, 0)]));
            }
        }
        if !caption.is_empty() {
            c.spawn((Text::new(caption), TextFont { font: k.f.italic.clone().into(), font_size: FontSize::Px(12.5), ..default() }, TextColor(SUBTLE), TextLayout::new(Justify::Center, bevy::text::LineBreak::WordBoundary), Node { max_width: Val::Px(680.), ..default() }));
        }
    });
}
