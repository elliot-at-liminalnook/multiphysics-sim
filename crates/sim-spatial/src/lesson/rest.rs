//! Lesson commands over the viewer's REST API. They go through the same
//! `Learn::act`, lesson edit layer (`sim_lesson::edit`) and note store as
//! clicks, so the UI, REST clients and the `sim-lesson` CLI share validation
//! and undo.
use super::*;
use serde::Deserialize;
use serde_json::{Value, json};

pub fn capabilities() -> Vec<Value> {
    use sim_api::capability as c;
    vec![
        c("lesson_list", json!({}), "Lessons in the lesson folder, in reading order (slug, title, category, scenes, parse errors)"),
        c("lesson_categories", json!({}), "Lessons grouped by category (categories.yaml order; uncategorised last), with which groups are folded in the list"),
        c("lesson_fold", json!({"category":"mechanisms"}), "Fold or unfold a category in the lesson list"),
        c("lesson_state", json!({}), "Screen, open lesson (revision, blocks with IDs and hashes), live scene (time, run key, fidelity, claims), notes, status"),
        c("lesson_open", json!({"slug":"worm-self-locking"}), "Open a lesson (its first scene becomes live) and show the lesson screen"),
        c("lesson_screen", json!({"learn":true}), "Show the lesson screen (true) or the builder on the live scene's sandbox copy (false)"),
        c("lesson_goto", json!({"block":"b3"}), "Scroll to a block or embed ID"),
        c("lesson_scene", json!({"scene":"hold","action":"seek","time":1.3}), "Live scene: activate/play/pause/restart/seek(time)/slider(parameter, value)/reset_sliders/explore/reset_sandbox. Recorded runs are cached by hash; seeking never re-simulates"),
        c("lesson_mode", json!({"mode":"annotate"}), "Page mode: read, annotate or edit"),
        c("lesson_edit", json!({"edit":{"edit":"replace_block","block":"b3","hash":"…","text":"New paragraph."},"expected_revision":"…","label":"Reword"}), "Edit lesson.md through the shared command layer: replace_block/insert_after/delete_block/replace_source. Needs the block hash; optional expected_revision (content hash). Edits that break the lesson are refused with file:line"),
        c("lesson_undo", json!({}), "Undo the last lesson edit (shared journal with the CLI)"),
        c("lesson_redo", json!({}), "Redo a lesson edit"),
        c("lesson_notes", json!({"action":{"operation":"list"}}), "Lesson notes: list/create(block|scene[,part,time_s],body)/reply/edit_comment/delete_comment/resolve/delete/undo/redo. Anchors re-attach after text edits; detached notes are kept. Optional expected_revision"),
        c("lesson_compare", json!({"id":"gearboxes"}), "Run a sim-compare block's saved study in the background (cached); read the table from lesson_state"),
        c("lesson_narration", json!({"action":{"action":"play"}}), "Narrated explainer (explainer.md): play/pause/stop/next/prev/section(index)/seek(time_s)/generate(section?). Without action: sections, audio state, timing kind, cue times and current marks. generate spends OpenRouter credit (estimate first; $1 ceiling per run)"),
        c("lesson_quiz", json!({"id":"current-follows-load","choice":0}), "Answer a lesson question like a reader would (choice index, text with a number for numeric/predict, or sketch: [[t, v], …] for a sketch), or reveal:true after two misses. Records progress and the spaced-review schedule; gates and prediction locks follow"),
        c("lesson_reflect", json!({"id":"why-stall-current","text":"…"}), "Save a self-explanation"),
        c("lesson_task", json!({"id":"fast-under-load","action":"start"}), "A sim-task: start (its own scene and sandbox), check (re-run the sandbox and judge the goal), hint (reveal the next hint). Results appear in lesson_state tasks"),
        c("lesson_lab", json!({"id":"knee-quarter","action":"tick","index":0}), "A sim-lab: tick (a checklist item), predict (text), run (ask the bench in SIM_BENCH_URL; refused until every item is ticked and a prediction is given)"),
        c("lesson_review", json!({"action":"start"}), "Mixed review session across lessons: start, next or end"),
        c("lesson_frames", json!({"scene":"load-step","mode":"seek","clock":"screen","count":64,"region":"card","tile_width":320,"path":"/tmp/sheet.png"}), "Contact sheet of a scene's animation: frames at times (seek: on the screen clock with pacing holds, or the sim clock; `times` list or `from`..`to` with `count` ≤ 256) or captured every `interval_s` while it plays for real (live: `start` scene or narration `section`, muted). One labelled grid PNG (index, screen time, sim time) as an image artifact, plus `path`; metadata lists caption, pace and times per tile"),
        c("lesson_settings", json!({"reduced_motion":true,"text_scale":1.15,"narration_speed":1.25,"transcript":true}), "Reader preferences (any subset); saved per machine"),
        c("lesson_progress", json!({}), "The learner's progress: answers, predictions, reflections, review due now, and which block the lesson is gated at"),
        c("lesson_ask", json!({"thread":"t-…"}), "Ask Codex about a lesson note (manual, read-only answer mode; the reply is posted as a comment)"),
    ]
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    LessonList,
    LessonCategories,
    LessonFold { category: String },
    LessonState,
    LessonOpen { slug: String },
    LessonScreen { learn: bool },
    LessonGoto { block: String },
    LessonScene {
        #[serde(default)]
        scene: Option<String>,
        action: String,
        #[serde(default)]
        time: Option<f64>,
        /// `slider`: the parameter and its new value (re-records like letting go).
        #[serde(default)]
        parameter: Option<String>,
        #[serde(default)]
        value: Option<f64>,
    },
    LessonMode { mode: PageMode },
    LessonEdit {
        edit: Edit,
        #[serde(default)]
        expected_revision: Option<String>,
        #[serde(default)]
        label: Option<String>,
    },
    LessonUndo,
    LessonRedo,
    LessonNotes {
        action: NotesRequest,
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    LessonCompare { id: String },
    LessonAsk { thread: String },
    LessonQuiz {
        id: String,
        /// Option index (choice) or text with a number (numeric, predict).
        #[serde(default)]
        choice: Option<usize>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        reveal: bool,
        /// Sketch questions: the drawn curve as [time, value] points.
        #[serde(default)]
        sketch: Option<Vec<[f64; 2]>>,
        /// Steps questions: the text for each blank step, in order.
        #[serde(default)]
        steps: Option<Vec<String>>,
        /// How sure: 0 a guess, 1 fairly sure, 2 sure.
        #[serde(default)]
        confidence: Option<u8>,
        /// Reveal the next hint instead of answering.
        #[serde(default)]
        hint: bool,
    },
    LessonTask {
        id: String,
        action: String,
    },
    LessonLab {
        id: String,
        action: String,
        #[serde(default)]
        index: Option<usize>,
        #[serde(default)]
        text: Option<String>,
    },
    LessonReview {
        action: String,
    },
    LessonSettings {
        #[serde(default)]
        reduced_motion: Option<bool>,
        #[serde(default)]
        text_scale: Option<f32>,
        #[serde(default)]
        narration_speed: Option<f32>,
        #[serde(default)]
        transcript: Option<bool>,
    },
    LessonReflect { id: String, text: String },
    LessonProgress,
    LessonNarration {
        #[serde(default)]
        action: Option<narrate::NarrateAction>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum NotesRequest {
    List,
    Create {
        #[serde(default)]
        block: Option<String>,
        #[serde(default)]
        scene: Option<String>,
        #[serde(default)]
        part: Option<String>,
        #[serde(default)]
        time_s: Option<f64>,
        body: String,
        #[serde(default)]
        author: Option<String>,
        #[serde(default)]
        title: Option<String>,
    },
    Reply {
        thread: String,
        body: String,
        #[serde(default)]
        author: Option<String>,
    },
    EditComment { thread: String, comment: String, body: String },
    DeleteComment { thread: String, comment: String },
    Resolve { thread: String, resolved: bool },
    Delete { thread: String },
    Undo,
    Redo,
}

pub fn state(learn: &Learn) -> Value {
    let lesson = learn.lesson.as_ref().map(|l| {
        json!({
            "slug": l.slug, "title": l.meta.title, "file": l.path, "revision": sim_lesson::edit::revision(&l.source),
            "blocks": l.blocks.iter().map(|b| json!({"id": b.id, "line": b.line, "end_line": b.end_line, "hash": b.hash, "section": b.section,
                "kind": b.kind.name()})).collect::<Vec<_>>(),
            "history": LessonStore::new(&l.path).history(),
        })
    });
    let scene = learn.scene.as_ref().map(|a| {
        json!({"id": a.id, "installed": a.installed, "time": a.time, "duration": a.duration(), "playing": a.playing, "recording": a.progress(), "error": a.error,
            "sandbox": a.sandbox, "run": a.run.as_ref().map(|r| json!({"key": r.key, "fidelity": r.fidelity, "frames": r.frames.len(), "checks": r.checks, "error": r.error, "applied": r.applied})),
            "presentation": {"caption": a.timeline.state_at(a.time).caption, "highlight": a.timeline.state_at(a.time).highlight}})
    });
    json!({
        "screen": if learn.screen_lesson() { "lesson" } else { "builder" }, "mode": learn.mode, "dir": learn.dir, "lesson": lesson, "lesson_error": learn.lesson_error,
        "scene": scene, "status": learn.status, "notes_revision": learn.notes_doc.revision, "notes": learn.notes_doc.threads.len(),
        "compares": learn.compares.iter().map(|(id, s)| (id.clone(), json!({"running": s.job.is_some(), "result": s.result.as_ref().map(|r| r.as_ref().map(|c| json!({"table": c.table, "variants": c.variants})).map_err(|e| e.clone()))}))).collect::<BTreeMap<_, _>>(),
        "ui_revision": learn.ui_revision, "agent": learn.agent.public(), "narration": narration_state(learn),
        "model": {"loading": learn.model.loading, "values": learn.model.values, "errors": learn.model.errors, "given": learn.model.given,
            "measured": learn.model.measured.iter().map(|(id, r)| (id.clone(), r.as_ref().map(|m| json!({"rms": m.rms, "max_gap": m.max_gap, "points": m.points, "fitted_to_data": m.fitted_to_data, "checks": m.checks})).map_err(|e| e.clone()))).collect::<BTreeMap<_, _>>(),
            "labs": learn.model.labs.iter().map(|(id, r)| (id.clone(), json!(r))).collect::<BTreeMap<_, _>>()},
        "tasks": learn.tasks.iter().map(|(id, t)| (id.clone(), json!({"hints": t.hints, "last": t.last.as_ref().map(|r| r.as_ref().map_err(|e| e.clone()))}))).collect::<BTreeMap<_, _>>(),
        "labs": learn.labs.iter().map(|(id, l)| (id.clone(), json!({"ticks": l.ticks, "prediction": l.prediction, "running": l.running, "result": l.result}))).collect::<BTreeMap<_, _>>(),
        "session": learn.session.as_ref().map(|(q, at)| json!({"queue": q, "at": at})),
        "settings": learn.settings,
        "open_question": learn.open_question(),
    })
}

fn author(learn: &Learn, a: Option<String>) -> String {
    a.filter(|a| !a.trim().is_empty()).unwrap_or_else(|| learn.author.clone())
}

pub fn execute(learn: &mut Learn, scene: &mut SpatialScene, command: &sim_api::Command) -> sim_api::Result {
    match sim_api::decode::<Request>(command)? {
        Request::LessonList => Ok(json!(learn.entries)),
        Request::LessonCategories => Ok(json!(sim_lesson::categories::group(&learn.entries, &learn.categories).iter().map(|g| json!({
            "id": g.category.id, "title": g.category.title, "summary": g.category.summary, "folded": learn.folded.contains(&g.category.id),
            "lessons": g.lessons.iter().map(|e| json!({"slug": e.slug, "title": e.title})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>())),
        Request::LessonFold { category } => {
            if !learn.folded.remove(&category) {
                learn.folded.insert(category);
            }
            learn.dirty = true;
            Ok(json!({"folded": learn.folded}))
        }
        Request::LessonState => Ok(state(learn)),
        Request::LessonOpen { slug } => {
            learn.open(&slug)?;
            learn.request_screen(true);
            Ok(state(learn))
        }
        Request::LessonScreen { learn: show } => {
            if !show {
                learn.try_act(LessonAction::OpenBuilder, scene)?;
                return Ok(state(learn));
            }
            learn.request_screen(true);
            Ok(state(learn))
        }
        Request::LessonGoto { block } => {
            learn.try_act(LessonAction::Goto(block), scene)?;
            Ok(state(learn))
        }
        Request::LessonScene { scene: id, action, time, parameter, value } => {
            let action = match action.as_str() {
                "activate" => LessonAction::Activate(id.ok_or("activate needs scene")?),
                "play" => LessonAction::Play,
                "pause" => LessonAction::Pause,
                "restart" => LessonAction::Restart,
                "reset_sandbox" => LessonAction::ResetSandbox,
                "seek" => {
                    let a = learn.scene.as_mut().ok_or("no live scene")?;
                    if a.run.is_none() {
                        return Err("the scene has not finished recording".into());
                    }
                    a.seek(time.ok_or("seek needs time")?);
                    a.playing = false;
                    learn.dirty = true;
                    return Ok(state(learn));
                }
                "slider" => {
                    let (parameter, value) = (parameter.ok_or("slider needs parameter")?, value.ok_or("slider needs value")?);
                    let a = learn.scene.as_mut().ok_or("no live scene")?;
                    let spec = a.scene.sliders.iter().find(|s| s.parameter == parameter).ok_or_else(|| format!("the scene has no slider for `{parameter}`"))?;
                    let v = spec.snap(value);
                    a.overrides.insert(parameter, v);
                    learn.rerecord();
                    return Ok(state(learn));
                }
                "reset_sliders" => LessonAction::ResetSliders,
                "explore" => LessonAction::Explore,
                other => return Err(format!("unknown scene action `{other}` (activate, play, pause, restart, seek, slider, reset_sliders, explore, reset_sandbox)")),
            };
            learn.try_act(action, scene)?;
            Ok(state(learn))
        }
        Request::LessonMode { mode } => {
            learn.try_act(LessonAction::Mode(mode), scene)?;
            Ok(state(learn))
        }
        Request::LessonEdit { edit, expected_revision, label } => {
            let applied = learn.edit(label.as_deref().unwrap_or("REST edit"), edit, expected_revision.as_deref())?;
            Ok(json!({"applied": applied, "state": state(learn)}))
        }
        Request::LessonUndo => learn.undo(false).map(|a| json!(a)),
        Request::LessonRedo => learn.undo(true).map(|a| json!(a)),
        Request::LessonNotes { action, expected_revision } => {
            if expected_revision.is_some_and(|r| r != learn.notes_doc.revision) {
                return Err("stale notes revision; read lesson_notes again".into());
            }
            let command = match action {
                NotesRequest::List => {
                    return Ok(json!({"revision": learn.notes_doc.revision, "threads": learn.threads(), "pending": learn.pending.len()}));
                }
                NotesRequest::Create { block, scene: s, part, time_s, body, author: who, title } => {
                    let anchor = match (block, s) {
                        (Some(b), None) => {
                            let lesson = learn.lesson.as_ref().ok_or("no lesson")?;
                            let section = lesson.block(&b).map(|x| x.section.clone()).ok_or_else(|| format!("no block `{b}`"))?;
                            LessonAnchor::Text { quote: learn.index.as_ref().and_then(|i| i.block_anchor(&b, &section)).ok_or("that block has no text")? }
                        }
                        (None, Some(s)) => LessonAnchor::Scene { scene: s, part, time_s, missing: false },
                        _ => return Err("give either block or scene".into()),
                    };
                    let body = body.trim().to_string();
                    let title = title.unwrap_or_else(|| sim_annotate::plain_comment(&body).lines().next().unwrap_or("Note").chars().take(80).collect());
                    let comment = Comment { id: sim_annotate::uid("c"), author: author(learn, who), body, created_at: sim_annotate::stamp(), edited_at: None, links: vec![] };
                    ThreadCommand::PutThread { thread: Thread { id: sim_annotate::uid("t"), title, resolved: false, targets: vec![anchor], comments: vec![comment], pin_m: None, view: None } }
                }
                NotesRequest::Reply { thread, body, author: who } => ThreadCommand::AddComment { thread, comment: Comment { id: sim_annotate::uid("c"), author: author(learn, who), body, created_at: sim_annotate::stamp(), edited_at: None, links: vec![] } },
                NotesRequest::EditComment { thread, comment, body } => ThreadCommand::EditComment { thread, comment, body, edited_at: sim_annotate::stamp() },
                NotesRequest::DeleteComment { thread, comment } => ThreadCommand::DeleteComment { thread, comment },
                NotesRequest::Resolve { thread, resolved } => ThreadCommand::Resolve { thread, resolved },
                NotesRequest::Delete { thread } => ThreadCommand::DeleteThread { id: thread },
                NotesRequest::Undo => ThreadCommand::Undo,
                NotesRequest::Redo => ThreadCommand::Redo,
            };
            let request = learn.note("REST note", command)?;
            Ok(json!({"submitted": request, "note": "applied off the UI thread; read lesson_notes (or lesson_state notes_revision) for the result"}))
        }
        Request::LessonCompare { id } => {
            learn.try_act(LessonAction::RunCompare(id), scene)?;
            Ok(state(learn))
        }
        Request::LessonTask { id, action } => {
            learn.try_act(match action.as_str() {
                "start" => LessonAction::TaskStart(id),
                "check" => LessonAction::TaskCheck(id),
                "hint" => LessonAction::TaskHint(id),
                other => return Err(format!("task action `{other}`: start, check or hint")),
            }, scene)?;
            Ok(state(learn))
        }
        Request::LessonLab { id, action, index, text } => {
            match action.as_str() {
                "tick" => learn.try_act(LessonAction::LabTick(id, index.ok_or("tick needs index")?), scene)?,
                "predict" => {
                    learn.labs.entry(id).or_default().prediction = text.ok_or("predict needs text")?;
                    learn.dirty = true;
                }
                "run" => learn.try_act(LessonAction::LabRun(id), scene)?,
                other => return Err(format!("lab action `{other}`: tick, predict or run")),
            }
            Ok(state(learn))
        }
        Request::LessonReview { action } => {
            learn.try_act(match action.as_str() {
                "start" => LessonAction::ReviewSession,
                "next" => LessonAction::ReviewNext,
                "end" => LessonAction::ReviewEnd,
                other => return Err(format!("review action `{other}`: start, next or end")),
            }, scene)?;
            Ok(state(learn))
        }
        Request::LessonSettings { reduced_motion, text_scale, narration_speed, transcript } => {
            if reduced_motion.is_some_and(|v| v != learn.settings.reduced_motion) {
                learn.try_act(LessonAction::Setting(super::Setting::ReducedMotion), scene)?;
            }
            if transcript.is_some_and(|v| v != learn.settings.transcript) {
                learn.try_act(LessonAction::Setting(super::Setting::Transcript), scene)?;
            }
            if let Some(v) = text_scale {
                learn.try_act(LessonAction::Setting(super::Setting::TextSize(v)), scene)?;
            }
            if let Some(v) = narration_speed {
                learn.try_act(LessonAction::Setting(super::Setting::NarrationSpeed(v)), scene)?;
            }
            Ok(json!(learn.settings))
        }
        Request::LessonQuiz { id, choice, text, reveal, sketch, steps, confidence, hint } => {
            if hint {
                learn.try_act(LessonAction::HintMore(id.clone()), scene)?;
                return Ok(json!({"record": learn.slug().and_then(|s| learn.progress.quiz(s, &id))}));
            }
            if let Some(c) = confidence {
                learn.confidence.remove(&id);
                learn.try_act(LessonAction::Confidence(id.clone(), c), scene)?;
            }
            if let Some(steps) = steps {
                let lesson = learn.lesson.as_ref().ok_or("no lesson")?;
                let q = lesson.quiz(&id).ok_or("no such question")?;
                let blanks: Vec<usize> = q.steps.iter().enumerate().filter(|(_, s)| s.blank()).map(|(i, _)| i).collect();
                for (i, t) in blanks.into_iter().zip(steps) {
                    learn.step_text.insert((id.clone(), i), t);
                }
            }
            if let Some(points) = sketch {
                // Drawn like a reader would: a stroke through the points (straight
                // between them) fills the columns it crosses.
                let q = learn.lesson.as_ref().and_then(|l| l.quiz(&id)).cloned().ok_or("no such question")?;
                let ([t0, t1], [lo, hi]) = (learn.sketch_window(&q), q.range.ok_or("not a sketch question")?);
                let n = super::practice::SKETCH_COLUMNS;
                let mut columns = vec![None; n];
                let mut points = points;
                points.sort_by(|a, b| a[0].total_cmp(&b[0]));
                for (c, column) in columns.iter_mut().enumerate() {
                    let t = t0 + (c as f64 + 0.5) / n as f64 * (t1 - t0);
                    let v = match points.iter().position(|p| p[0] >= t) {
                        Some(0) if points[0][0] > t => None,
                        Some(0) => Some(points[0][1]),
                        Some(k) => {
                            let ([ta, va], [tb, vb]) = (points[k - 1], points[k]);
                            Some(va + (vb - va) * (t - ta) / (tb - ta).max(1e-15))
                        }
                        None => None,
                    };
                    *column = v.map(|v| (((v - lo) / (hi - lo)).clamp(0., 1.)) as f32);
                }
                learn.sketches.insert(id.clone(), columns);
            }
            if reveal {
                learn.try_act(LessonAction::QuizReveal(id.clone()), scene)?;
            } else {
                if let Some(i) = choice {
                    learn.try_act(LessonAction::QuizPick(id.clone(), i), scene)?;
                }
                if let Some(t) = text {
                    learn.quiz_text.insert(id.clone(), t);
                }
                learn.try_act(LessonAction::QuizCheck(id.clone()), scene)?;
            }
            Ok(json!({"verdict": learn.quiz_verdict.get(&id), "record": learn.slug().and_then(|s| learn.progress.quiz(s, &id)), "gate": learn.gate()}))
        }
        Request::LessonReflect { id, text } => {
            learn.input = Some(Input { purpose: Purpose::Reflection(id.clone()), buffer: text });
            learn.try_act(LessonAction::ReflectSave(id), scene)?;
            Ok(json!({"saved": true}))
        }
        Request::LessonProgress => Ok(json!({"progress": learn.progress, "due": learn.progress.due(sim_lesson::progress::now()), "gate": learn.gate(), "gate_block": learn.gate().and_then(|g| learn.lesson.as_ref().and_then(|l| l.blocks.get(g.saturating_sub(1)).map(|b| b.id.clone())))})),
        Request::LessonNarration { action } => {
            if let Some(a) = action {
                learn.try_act(LessonAction::Narrate(a), scene)?;
            }
            Ok(narration_state(learn))
        }
        Request::LessonAsk { thread } => {
            learn.thread = Some(thread);
            learn.try_act(LessonAction::Ask, scene)?;
            Ok(state(learn))
        }
    }
}

pub fn narration_state(learn: &Learn) -> Value {
    let Some(n) = &learn.narration else { return json!({"explainer": null}) };
    let plan = sim_voice::plan(&n.explainer);
    json!({
        "explainer": n.explainer.path, "playing": n.playing, "section": n.section, "time": n.time, "waiting": n.wait.as_ref().map(|w| format!("{w:?}")),
        "audio": learn.player.as_ref().map(|p| { let st = p.state(); json!({"loaded": st.loaded, "position": st.position, "playing": st.playing, "finished": st.finished, "error": st.error}) }), "loaded": n.loaded,
        "marks": n.marks, "subtitle": n.subtitle(), "job": n.job.as_ref().map(|j| j.progress()), "last_report": n.last_report,
        "sections": n.explainer.sections.iter().enumerate().map(|(i, s)| json!({"id": s.id, "title": s.title, "status": plan[i].status, "estimated_usd": plan[i].estimated_usd, "timing": n.timings[i].kind, "duration_s": n.timings[i].duration_s, "cues": s.cues.iter().zip(&n.times[i]).map(|(c, t)| json!({"at_s": t, "cue": c.cue})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
    })
}
