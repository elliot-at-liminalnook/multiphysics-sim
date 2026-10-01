use super::*;
use std::time::{Duration, Instant};

/// The viewer's edits and notes go through the shared lesson command
/// layer and note store; a note stays on its paragraph after edits above it.
#[test]
fn learn_edits_undo_and_notes_share_the_command_layers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("learn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lesson_dir = dir.join("lessons/motor-torque-speed");
    std::fs::create_dir_all(&lesson_dir).unwrap();
    for f in ["lesson.md", "load-step.rhai", "motor.system.json"] {
        std::fs::copy(root.join("lessons/motor-torque-speed").join(f), lesson_dir.join(f)).unwrap();
    }
    // SAFETY: this test binary sets these once; nothing else reads them concurrently.
    unsafe {
        std::env::set_var("SIM_LESSON_CACHE", dir.join("cache"));
        std::env::set_var("SIM_LESSON_SANDBOX", dir.join("sandbox"));
    }
    let mut learn = Learn::new(dir.join("lessons"), root.join("library/systems"), sim_runtime::registry());
    assert_eq!(learn.entries.len(), 1);
    learn.open("motor-torque-speed").unwrap();
    let original = learn.lesson.clone().unwrap();
    // A note on the first paragraph.
    let b = original.blocks.iter().find(|b| matches!(b.kind, BlockKind::Markdown { .. })).unwrap().clone();
    let quote = learn.index.as_ref().unwrap().block_anchor(&b.id, &b.section).unwrap();
    let thread = Thread { id: "t".into(), title: "Question".into(), resolved: false, targets: vec![LessonAnchor::Text { quote }], comments: vec![Comment { id: "c".into(), author: "Reader".into(), body: "Why?".into(), created_at: sim_annotate::stamp(), edited_at: None, links: vec![] }], pin_m: None, view: None };
    let id = learn.note("New note", ThreadCommand::PutThread { thread }).unwrap();
    let start = Instant::now();
    loop {
        if let Some(r) = learn.notes.as_mut().unwrap().result(id) {
            learn.notes_doc = r.unwrap();
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(learn.notes_doc.revision, 1);
    // Insert a block at the top: the note moves with its paragraph.
    learn.edit("Add", Edit::InsertAfter { block: None, text: "A new opening paragraph.".into() }, None).unwrap();
    let moved = learn.threads()["t"].targets[0].block(learn.index.as_ref().unwrap()).map(String::from);
    let expected = learn.lesson.as_ref().unwrap().blocks.iter().find(|x| x.text(&learn.lesson.as_ref().unwrap().source) == b.text(&original.source)).map(|x| x.id.clone());
    assert_eq!(moved, expected);
    assert_ne!(moved.as_deref(), Some(b.id.as_str()), "block IDs shifted, the anchor followed the text");
    // Undo restores the file exactly.
    learn.undo(false).unwrap();
    assert_eq!(learn.lesson.as_ref().unwrap().source, original.source);
    drop(learn);
    let _ = std::fs::remove_dir_all(dir);
}

/// Mastery pacing: a wrong answer keeps the rest locked, the right one
/// opens the next part; a prediction unlocks its scene; figures load.
#[test]
fn questions_gate_the_lesson_and_predictions_unlock_scenes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("learn-practice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lesson_dir = dir.join("lessons/motor-torque-speed");
    std::fs::create_dir_all(&lesson_dir).unwrap();
    for f in ["lesson.md", "load-step.rhai", "motor.system.json", "motor-model.svg", "torque-speed.svg"] {
        std::fs::copy(root.join("lessons/motor-torque-speed").join(f), lesson_dir.join(f)).unwrap();
    }
    let mut learn = Learn::new(dir.join("lessons"), root.join("library/systems"), sim_runtime::registry());
    learn.progress_path = dir.join("progress.json");
    learn.progress = sim_lesson::progress::Progress::default();
    learn.open("motor-torque-speed").unwrap();
    assert!(learn.scene.is_none(), "the only scene waits for its prediction");
    assert_eq!(learn.scene_locked("load-step").as_deref(), Some("predict-doubled"));
    let blocks = learn.lesson.as_ref().unwrap().blocks.clone();
    // One idea at a time: each opening question opens the next part.
    assert_eq!(blocks[learn.gate().unwrap() - 1].id, "two-sides");
    learn.practice(LessonAction::QuizPick("two-sides".into(), 1)).unwrap();
    learn.practice(LessonAction::QuizCheck("two-sides".into())).unwrap();
    for (id, typed) in [("torque-from-current", "36 mN·m"), ("back-emf-at-speed", "3 V")] {
        assert_eq!(blocks[learn.gate().unwrap() - 1].id, id);
        learn.quiz_text.insert(id.into(), typed.into());
        learn.practice(LessonAction::QuizCheck(id.into())).unwrap();
        assert!(learn.quiz_verdict[id].correct, "{id}: {}", learn.quiz_verdict[id].feedback);
    }
    // The faded worked example: the blank steps, with units.
    assert_eq!(blocks[learn.gate().unwrap() - 1].id, "budget-steps");
    learn.step_text.insert(("budget-steps".into(), 1), "4.8 V".into());
    learn.step_text.insert(("budget-steps".into(), 2), "2400 mA".into());
    learn.practice(LessonAction::QuizCheck("budget-steps".into())).unwrap();
    assert!(learn.quiz_verdict["budget-steps"].correct, "{}", learn.quiz_verdict["budget-steps"].feedback);
    // A hint, then a sure answer: recorded with the hint and the confidence.
    assert_eq!(blocks[learn.gate().unwrap() - 1].id, "current-at-speed");
    learn.practice(LessonAction::HintMore("current-at-speed".into())).unwrap();
    learn.practice(LessonAction::Confidence("current-at-speed".into(), 2)).unwrap();
    learn.quiz_text.insert("current-at-speed".into(), "1.5 A".into());
    learn.practice(LessonAction::QuizCheck("current-at-speed".into())).unwrap();
    let r = learn.progress.quiz("motor-torque-speed", "current-at-speed").unwrap();
    assert_eq!((r.history[0].hints, r.history[0].confidence, r.box_), (1, Some(sim_lesson::progress::Confidence::Sure), 0), "a hinted answer starts in the first box");
    let first_gate = learn.gate().unwrap();
    assert_eq!(blocks[first_gate - 1].id, "stall-current");
    // A wrong answer keeps it locked and explains the misconception.
    learn.practice(LessonAction::QuizPick("stall-current".into(), 0)).unwrap();
    learn.practice(LessonAction::QuizCheck("stall-current".into())).unwrap();
    assert!(!learn.quiz_verdict["stall-current"].correct);
    assert!(learn.quiz_verdict["stall-current"].feedback.contains("no back-EMF"));
    assert_eq!(learn.gate(), Some(first_gate));
    learn.practice(LessonAction::QuizPick("stall-current".into(), 1)).unwrap();
    learn.practice(LessonAction::QuizCheck("stall-current".into())).unwrap();
    let second = learn.gate().unwrap();
    assert_eq!(blocks[second - 1].id, "no-load-speed");
    // A varied question: this attempt's supply voltage, k from the model (read off the UI thread).
    let mut images = Assets::<Image>::default();
    let start = std::time::Instant::now();
    while !learn.model.given.contains_key("no-load-speed") {
        learn.poll_model(&mut images);
        assert!(start.elapsed() < std::time::Duration::from_secs(60), "model values: {:?}", learn.model.errors);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let v = learn.progress.quiz("motor-torque-speed", "no-load-speed").unwrap().values["V"];
    let rpm = v / 0.012 * 60. / std::f64::consts::TAU;
    learn.quiz_text.insert("no-load-speed".into(), format!("{rpm:.0} rpm"));
    learn.practice(LessonAction::QuizCheck("no-load-speed".into())).unwrap();
    assert!(learn.quiz_verdict["no-load-speed"].correct, "{} V: {}", v, learn.quiz_verdict["no-load-speed"].feedback);
    assert!(learn.model.values.contains_key("param:motor.torque_constant"), "prose numbers resolved too");
    assert_eq!(blocks[learn.gate().unwrap() - 1].id, "line-midpoint");
    learn.practice(LessonAction::QuizPick("line-midpoint".into(), 0)).unwrap();
    learn.practice(LessonAction::QuizCheck("line-midpoint".into())).unwrap();
    let third = learn.gate().unwrap();
    assert_eq!(blocks[third - 1].id, "current-follows-load", "the prediction does not gate the text");
    // The prediction unlocks the scene and is saved.
    learn.quiz_text.insert("predict-doubled".into(), "500".into());
    learn.practice(LessonAction::QuizCheck("predict-doubled".into())).unwrap();
    assert!(learn.scene_locked("load-step").is_none());
    assert_eq!(learn.scene.as_ref().map(|a| a.id.as_str()), Some("load-step"));
    let saved = sim_lesson::progress::Progress::load(&dir.join("progress.json"));
    assert_eq!(saved.quiz("motor-torque-speed", "predict-doubled").and_then(|r| r.prediction.clone()).as_deref(), Some("500"));
    assert!(saved.passed("motor-torque-speed", "stall-current"));
    // Figures rasterize off the calling thread.
    let start = std::time::Instant::now();
    while learn.figures.values().any(|f| matches!(f.state, practice::FigureState::Loading(_))) {
        learn.poll_figures(&mut images);
        assert!(start.elapsed() < std::time::Duration::from_secs(20));
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(learn.figures.len(), 2);
    assert!(learn.figures.values().all(|f| matches!(f.state, practice::FigureState::Ready { .. })));
    drop(learn);
    let _ = std::fs::remove_dir_all(dir);
}

/// Draw the whole lesson through Bevy with its cards in every state
/// (unanswered, typing, answered, revealed, writing a reflection, all
/// unlocked): spawning must never produce an invalid bundle.
#[test]
fn every_card_state_spawns_valid_ui() {
    use bevy::ecs::system::RunSystemOnce;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("learn-draw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lessons")).unwrap();
    std::fs::copy(root.join("lessons/concepts.yaml"), dir.join("lessons/concepts.yaml")).unwrap();
    for slug in ["motor-torque-speed", "worm-self-locking", "motor-driver-board", "knee-servo-measured"] {
        let from = root.join("lessons").join(slug);
        let to = dir.join("lessons").join(slug);
        std::fs::create_dir_all(&to).unwrap();
        for e in std::fs::read_dir(&from).unwrap().flatten() {
            if e.path().is_file() && !e.file_name().to_string_lossy().contains("annotations") {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
        // Systems referenced outside the lesson folder.
        let text = std::fs::read_to_string(to.join("lesson.md")).unwrap().replace("../../examples/", &format!("{}/examples/", root.display()));
        std::fs::write(to.join("lesson.md"), text).unwrap();
    }
    let mut world = World::new();
    world.insert_resource(crate::tests::fixture());
    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(crate::ui_kit::UiFonts { regular: Handle::default(), italic: Handle::default(), mono: Handle::default(), icons: Default::default(), medium: Handle::default(), semibold: Handle::default() });
    let mut learn = Learn::new(dir.join("lessons"), root.join("library/systems"), sim_runtime::registry());
    learn.progress_path = dir.join("progress.json");
    learn.progress = sim_lesson::progress::Progress::default();
    world.insert_resource(learn);
    let draw = |world: &mut World| {
        world.resource_mut::<Learn>().dirty = true;
        world.run_system_once(ui::rebuild).unwrap();
        world.flush();
    };
    world.resource_mut::<Learn>().refresh_catalog();
    for slug in ["motor-torque-speed", "worm-self-locking", "motor-driver-board", "knee-servo-measured"] {
        world.resource_mut::<Learn>().open(slug).unwrap();
        draw(&mut world);
        // Model values (numbers, equations, measured data, lab predictions) arrive, then draw.
        {
            let start = std::time::Instant::now();
            let mut images = Assets::<Image>::default();
            while world.resource::<Learn>().model.loading {
                world.resource_mut::<Learn>().poll_model(&mut images);
                assert!(start.elapsed() < std::time::Duration::from_secs(120), "{slug}: model");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            assert!(world.resource::<Learn>().model.errors.is_empty(), "{slug}: {:?}", world.resource::<Learn>().model.errors);
        }
        draw(&mut world);
        let quizzes: Vec<sim_lesson::quiz::Quiz> = world.resource::<Learn>().lesson.as_ref().unwrap().quizzes().map(|(_, q)| q.clone()).collect();
        for q in &quizzes {
            // Typing (numeric and predict) or picking (choice), then a wrong and a right answer.
            {
                let mut l = world.resource_mut::<Learn>();
                if q.kind == sim_lesson::quiz::QuizKind::Steps {
                    l.practice(LessonAction::StepInput(q.id.clone(), q.steps.iter().position(|s| s.blank()).unwrap())).unwrap();
                    l.input.as_mut().unwrap().buffer = "1".into();
                } else if q.options.is_empty() {
                    l.practice(LessonAction::QuizInput(q.id.clone())).unwrap();
                    l.input.as_mut().unwrap().buffer = "123 units".into();
                } else {
                    l.practice(LessonAction::QuizPick(q.id.clone(), 0)).unwrap();
                }
            }
            draw(&mut world);
            {
                let mut l = world.resource_mut::<Learn>();
                let _ = l.practice(LessonAction::QuizCheck(q.id.clone()));
                let _ = l.practice(LessonAction::QuizCheck(q.id.clone()));
            }
            draw(&mut world);
            {
                let mut l = world.resource_mut::<Learn>();
                match q.options.iter().position(|o| o.correct) {
                    Some(i) => l.practice(LessonAction::QuizPick(q.id.clone(), i)).unwrap(),
                    None => {
                        l.quiz_text.insert(q.id.clone(), q.answer.unwrap_or(1.0).to_string());
                    }
                }
                if q.kind == sim_lesson::quiz::QuizKind::Predict {
                    let _ = l.practice(LessonAction::QuizCheck(q.id.clone()));
                } else if !q.pretest {
                    let _ = l.practice(LessonAction::HintMore(q.id.clone()));
                    l.practice(LessonAction::QuizReveal(q.id.clone())).unwrap();
                }
            }
            draw(&mut world);
        }
        let reflections: Vec<String> = world.resource::<Learn>().lesson.as_ref().unwrap().blocks.iter().filter_map(|b| match &b.kind { BlockKind::Reflect(r) => Some(r.id.clone()), _ => None }).collect();
        for r in reflections {
            {
                let mut l = world.resource_mut::<Learn>();
                l.practice(LessonAction::ReflectInput(r.clone())).unwrap();
                l.input.as_mut().unwrap().buffer = "Because the back-EMF is zero at stall.".into();
            }
            draw(&mut world);
            world.resource_mut::<Learn>().practice(LessonAction::ReflectSave(r)).unwrap();
            draw(&mut world);
        }
        assert_eq!(world.resource::<Learn>().gate(), None, "{slug}: everything unlocked");
        // Tasks (started, with a hint) and labs (ticked, predicted) draw in every state.
        let (tasks, labs): (Vec<String>, Vec<String>) = {
            let l = world.resource::<Learn>();
            let lesson = l.lesson.as_ref().unwrap();
            (lesson.blocks.iter().filter_map(|b| match &b.kind { BlockKind::Task(t) => Some(t.id.clone()), _ => None }).collect(), lesson.blocks.iter().filter_map(|b| match &b.kind { BlockKind::Lab(x) => Some(x.id.clone()), _ => None }).collect())
        };
        for t in tasks {
            let mut l = world.resource_mut::<Learn>();
            l.start_task(&t).unwrap();
            l.tasks.entry(t.clone()).or_default().hints = 2;
            drop(l);
            draw(&mut world);
        }
        for x in labs {
            let mut l = world.resource_mut::<Learn>();
            for i in 0..4 {
                let mut scene = crate::tests::fixture();
                l.try_act(LessonAction::LabTick(x.clone(), i), &mut scene).unwrap();
            }
            l.labs.entry(x.clone()).or_default().prediction = "0.5 rad/s".into();
            l.labs.entry(x.clone()).or_default().result = Some(Ok(serde_json::json!({"steady_rad_s": 0.44, "stopped": null, "receipt": "r.json"})));
            drop(l);
            draw(&mut world);
        }
        // Edit mode and annotate mode draw too.
        world.resource_mut::<Learn>().mode = PageMode::Edit;
        draw(&mut world);
        world.resource_mut::<Learn>().mode = PageMode::Annotate;
        draw(&mut world);
        world.resource_mut::<Learn>().mode = PageMode::Read;
    }
    world.remove_resource::<Learn>();
    let _ = std::fs::remove_dir_all(dir);
}
