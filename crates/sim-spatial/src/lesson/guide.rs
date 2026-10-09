//! `lesson_guide` (and `GET /v1/lesson_guide`): how Lessons mode works, for
//! an agent starting cold. Concepts, workflows in order, every `lesson_*`
//! command with a working example, and the rules. Every command's full
//! description and refusals are in `GET /v1/capabilities`; `lesson_state`
//! shows what the page shows.
use serde_json::{Value, json};

/// The guide (`topic` narrows it to one section).
pub(crate) fn guide(topic: Option<&str>) -> Result<Value, String> {
    let all = json!({
        "about": "Lessons mode teaches with live systems. A lesson is a folder <slug>/lesson.md: Markdown with YAML front matter and fenced blocks. Scenes (sim-scene) are system runs recorded on the same runtime Build mode uses and animated on the page; their expect claims are checked on the recording. The lesson page is drawn over the builder, so a scene can be opened as an editable sandbox copy. Lessons answers to the lesson_* commands in Lessons mode and in Build mode.",
        "how_to_call": {
            "batch": "POST /v1/batch {\"commands\":[{\"command\":NAME,\"args\":{...}}]} → {\"job_id\"}; GET /v1/jobs/{job_id} until it ends; results[i].ok / value / error. Commands in one batch run in order.",
            "resources": "GET /v1/lesson_guide (this), /v1/capabilities (every command), /v1/lesson (the same answer as lesson_state, refreshed as the page changes).",
            "entering": "viewer_mode {mode: lessons, path: /abs/lessons} (a folder of <slug>/lesson.md), or launch sim-spatial --lessons DIR. Then lesson_list, then lesson_open {slug}. Without lessons open every lesson_* command except this guide is refused, naming how to open them.",
            "seeing": "lesson_state for the page, the open lesson's blocks (IDs, lines, hashes) and the live scene; lesson_frames for a contact sheet of a scene's animation (one PNG of labelled frames); screenshot {path} for the window as drawn.",
        },
        "concepts": {
            "blocks": "Every block of lesson.md has an ID (b1, b2, …), a line range and a content hash, listed in lesson_state.lesson.blocks. Embeds carry their own IDs (a scene's id, a quiz's id). Kinds: sim-scene, sim-component (a library card), sim-compare (a saved study), sim-quiz, sim-recall, sim-reflect, sim-equation, sim-remedy, sim-task, sim-lab, sim-measured.",
            "scenes": "A scene is a system run recorded once and cached by a hash of everything that shapes it; seeking and replay never re-simulate. A slider re-records the scene at the new parameter value. lesson_state.scene.run.checks holds each expect claim's result on that recording.",
            "sandbox": "lesson_screen {learn: false} shows the builder on the live scene's sandbox copy, which can be edited and run like any system; reset_sandbox restores the lesson's version. The lesson file is unchanged by sandbox edits.",
            "editing": "lesson_edit changes lesson.md through the shared edit layer (the same journal as the sim-lesson CLI, so undo works across both). An edit names the block's hash; expected_revision guards against editing a file that changed. An edit that would break the lesson is refused with file:line.",
            "learner_state": "Quizzes, predictions, reflections and review schedules are the learner's progress, kept in runs/lessons/progress.json ($SIM_LESSON_PROGRESS overrides it). An agent's answers count as the learner's, so answer only when asked to. Gates hold the page at a block until its question is answered.",
            "notes": "Notes anchor to a block or to a scene (and a part and time in it). They re-attach after text edits; a note whose text is gone is kept as detached.",
            "costs_and_safety": "lesson_narration generate spends OpenRouter credit (estimate first; a $1 ceiling per run). lesson_lab run moves real hardware and is refused to automation: an operator at the window runs it. lesson_ask asks Codex and posts the answer as a comment.",
        },
        "workflows": [
            {"goal": "Read a lesson", "steps": [
                "lesson_list (or lesson_categories) to find the slug",
                "lesson_open {slug}; lesson_state shows its blocks and the live scene",
                "lesson_goto {block} to bring a block into view; lesson_scene {action: play} or {action: seek, time} to watch a scene",
                "lesson_frames {scene, mode: seek, count: 16, path} for a contact sheet when the animation itself matters"]},
            {"goal": "Answer as a learner", "steps": [
                "lesson_state.open_question names the question the page waits on",
                "lesson_quiz {id, choice} (or text, sketch, steps); reveal: true after two misses; hint: true for the next hint",
                "lesson_reflect {id, text} for a self-explanation; lesson_progress for the record"]},
            {"goal": "Explore a scene", "steps": [
                "lesson_scene {scene, action: slider, parameter, value} re-records at the new value",
                "lesson_scene {action: explore} or lesson_screen {learn: false} opens the sandbox copy in the builder",
                "lesson_scene {action: reset_sandbox} or {action: reset_sliders} to return to the lesson's version"]},
            {"goal": "Edit a lesson", "steps": [
                "lesson_mode {mode: edit}",
                "lesson_state.lesson.blocks gives the block's hash and lesson.revision",
                "lesson_edit {edit: {edit: replace_block, block, hash, text}, expected_revision, label}",
                "lesson_state.scene.run.checks after the scene re-records; lesson_undo / lesson_redo if needed"]},
            {"goal": "Leave a note", "steps": [
                "lesson_notes {action: {operation: create, block: b3, body}} (or scene, part, time_s)",
                "lesson_notes {action: {operation: list}} to read threads; reply, resolve and delete work the same way"]},
        ],
        "commands": {
            "lesson_guide": {"example": {"topic": "workflows"}, "does": "This guide; topic narrows it."},
            "lesson_list": {"example": {}, "does": "Lessons in reading order (slug, title, category, scenes, parse errors)."},
            "lesson_categories": {"example": {}, "does": "Lessons grouped by category, with which groups are folded."},
            "lesson_fold": {"example": {"category": "mechanisms"}, "does": "Fold or unfold a category in the list."},
            "lesson_state": {"example": {}, "does": "Screen, open lesson (revision, blocks), live scene (time, run, claims), notes, compares, tasks, labs, narration, settings."},
            "lesson_open": {"example": {"slug": "worm-self-locking"}, "does": "Open a lesson; its first scene becomes live."},
            "lesson_screen": {"example": {"learn": true}, "does": "The lesson page (true) or the builder on the scene's sandbox (false)."},
            "lesson_goto": {"example": {"block": "b3"}, "does": "Scroll to a block or embed ID."},
            "lesson_scene": {"example": {"scene": "hold", "action": "seek", "time": 1.3}, "does": "activate, play, pause, restart, seek, slider, reset_sliders, explore, reset_sandbox."},
            "lesson_mode": {"example": {"mode": "annotate"}, "does": "Page mode: read, annotate or edit."},
            "lesson_edit": {"example": {"edit": {"edit": "replace_block", "block": "b3", "hash": "…", "text": "New paragraph."}, "expected_revision": "…", "label": "Reword"}, "does": "Edit lesson.md: replace_block, insert_after, delete_block, replace_source."},
            "lesson_undo": {"example": {}, "does": "Undo the last lesson edit (journal shared with the CLI)."},
            "lesson_redo": {"example": {}, "does": "Redo a lesson edit."},
            "lesson_notes": {"example": {"action": {"operation": "list"}}, "does": "list, create, reply, edit_comment, delete_comment, resolve, delete, undo, redo."},
            "lesson_compare": {"example": {"id": "gearboxes"}, "does": "Run a sim-compare block's saved study in the background; the table appears in lesson_state.compares."},
            "lesson_narration": {"example": {"action": {"action": "play"}}, "does": "The narrated explainer: play, pause, stop, next, prev, section, seek, generate (paid)."},
            "lesson_quiz": {"example": {"id": "current-follows-load", "choice": 0}, "does": "Answer a question as a reader would; records progress."},
            "lesson_reflect": {"example": {"id": "why-stall-current", "text": "…"}, "does": "Save a self-explanation."},
            "lesson_task": {"example": {"id": "fast-under-load", "action": "start"}, "does": "A sim-task: start, check, hint."},
            "lesson_lab": {"example": {"id": "knee-quarter", "action": "tick", "index": 0}, "does": "A sim-lab: tick, predict; run is for an operator at the window only."},
            "lesson_review": {"example": {"action": "start"}, "does": "A mixed review session across lessons: start, next, end."},
            "lesson_frames": {"example": {"scene": "load-step", "mode": "seek", "clock": "screen", "count": 64, "region": "card", "tile_width": 320, "path": "/tmp/sheet.png"}, "does": "Contact sheet of a scene's animation (an image artifact)."},
            "lesson_settings": {"example": {"reduced_motion": true, "text_scale": 1.15, "narration_speed": 1.25, "transcript": true}, "does": "Reader preferences, saved per machine."},
            "lesson_progress": {"example": {}, "does": "Answers, predictions, reflections, review due, and where the lesson is gated."},
            "lesson_ask": {"example": {"thread": "t-…"}, "does": "Ask Codex about a note; the answer is posted as a comment."},
        },
        "rules": [
            "Read lesson_state before and after acting; a refusal names its reason (no lesson open, a stale hash, a gate, a busy builder).",
            "Edit with the block's current hash and the lesson's revision; never write lesson.md behind the edit layer while it is open.",
            "A claim that fails is a finding about the lesson or the model, not something to hide: report it or fix the cause.",
            "Do not generate narration or ask Codex without the person's say-so: both spend money or credits.",
            "Never try to run a lab: physical motion needs an operator present.",
        ],
    });
    match topic {
        None => Ok(all),
        Some(t) => all.get(t).cloned().map(|v| json!({t: v})).ok_or_else(|| format!("no guide topic {t} (about, how_to_call, concepts, workflows, commands, rules)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions::Action;
    use crate::lesson::actions::LessonCommand;

    /// Every lesson command is in the guide, every guide command exists,
    /// and every example parses as the command it documents.
    #[test]
    fn the_guide_lists_every_lesson_command() {
        let all = guide(None).unwrap();
        let listed: std::collections::BTreeSet<String> = all["commands"].as_object().unwrap().keys().cloned().collect();
        let specs: std::collections::BTreeSet<String> = LessonCommand::commands().into_iter().map(|s| s.name.to_string()).collect();
        assert_eq!(listed, specs);
        for (name, c) in all["commands"].as_object().unwrap() {
            let command = sim_api::Command { command: name.clone(), args: c["example"].clone() };
            assert!(LessonCommand::parse(&command).is_ok(), "{name}: {:?}", LessonCommand::parse(&command).err());
        }
        assert_eq!(guide(Some("rules")).unwrap()["rules"], all["rules"]);
        assert!(guide(Some("nope")).is_err());
    }
}
