//! Drafting a lesson with an assistant, without letting it write anything
//! unchecked. The assistant (Codex through `sim-agent`, read-only) gets the
//! authoring guide (`lessons/AUTHORING_PROMPT.md`), the system's instances,
//! parameters (with provenance) and observables, an example lesson and the
//! author's objectives, and returns the text of `lesson.md`. The host writes
//! it only under `lessons/drafts/<slug>/`, runs `sim-lesson check`, and sends
//! the findings back for another round until the draft is clean or the
//! rounds run out. Promoting a draft into `lessons/` is the author's move.
use crate::lesson::{self, CheckOptions, Severity};
use serde_json::{Value, json};
use sim_core::BehaviorRegistry;
use std::path::{Path, PathBuf};

pub struct DraftRequest {
    pub slug: String,
    /// The system file the lesson teaches with.
    pub system: PathBuf,
    /// What the reader should be able to do afterwards.
    pub objectives: String,
    /// The lessons folder (for the guide, an example and the drafts folder).
    pub lessons: PathBuf,
    pub rounds: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DraftOutcome {
    pub path: PathBuf,
    pub rounds: usize,
    /// Findings of the last check (empty: clean).
    pub findings: Vec<String>,
    pub errors: usize,
}

fn relative(from_dir: &Path, to: &Path) -> String {
    // Both absolute: walk up from `from_dir` to the common ancestor.
    let (from, to) = (from_dir.components().collect::<Vec<_>>(), to.components().collect::<Vec<_>>());
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for c in &to[common..] {
        out.push(c.as_os_str());
    }
    out.display().to_string()
}

/// What the assistant is told: the context for a draft.
pub fn context(req: &DraftRequest, registry: &BehaviorRegistry) -> Result<(String, Value), String> {
    let guide = std::fs::read_to_string(req.lessons.join("AUTHORING_PROMPT.md")).map_err(|e| format!("{}: {e}", req.lessons.join("AUTHORING_PROMPT.md").display()))?;
    let dir = std::fs::canonicalize(&req.lessons).map_err(|e| e.to_string())?.join("drafts").join(&req.slug);
    let system = std::fs::canonicalize(&req.system).map_err(|e| format!("{}: {e}", req.system.display()))?;
    let doc = lesson::load_system(&system, registry)?;
    let compiled = crate::system_builder::compile(&doc, registry, crate::system_builder::config_for(&doc))?;
    let d = &compiled.description;
    let observables: Vec<String> = d.observables.keys().map(|id| crate::system_builder::observable_key(d, id)).collect();
    let origins = crate::lesson_model::origins(&doc);
    let components: serde_json::Map<String, Value> = d.components.iter().map(|(p, c)| (p.clone(), json!({"type": c.component_type, "label": c.label, "parameters": c.parameters.iter().map(|(k, v)| (k.clone(), json!({"value": v.value, "unit": v.unit}))).collect::<serde_json::Map<_, _>>()}))).collect();
    let example = std::fs::read_to_string(req.lessons.join("motor-torque-speed/lesson.md")).unwrap_or_default();
    let concepts = std::fs::read_to_string(req.lessons.join("concepts.yaml")).unwrap_or_default();
    let context = json!({
        "objectives": req.objectives,
        "slug": req.slug,
        "system_path_for_front_matter": relative(&dir, &system),
        "system": {"title": doc.title, "components": components, "observables": observables, "origins": origins},
        "concepts_yaml": concepts,
        "example_lesson": example,
    });
    Ok((guide, context))
}

/// The question for the first round and for revisions.
pub fn question(req: &DraftRequest, previous: Option<(&str, &[String])>) -> String {
    match previous {
        None => format!("Write the complete lesson.md for a new lesson `{}` that teaches with the system described in the context. Objectives: {}. Use `system_path_for_front_matter` for the system file in the front matter. Return only the file's text in `body` (no code fence around it) and an empty `links` list.", req.slug, req.objectives),
        Some((draft, findings)) => format!("`sim-lesson check` found problems in your draft of `{}`. Fix every one and return the complete corrected lesson.md in `body` (links empty).\nFindings:\n{}\nYour draft:\n{draft}", req.slug, findings.join("\n")),
    }
}

/// Run the draft loop. `ask(instructions, question, context)` returns the
/// assistant's body. Only `lessons/drafts/<slug>/lesson.md` is written.
pub fn draft(req: &DraftRequest, registry: &BehaviorRegistry, mut ask: impl FnMut(&str, &str, &Value) -> Result<String, String>) -> Result<DraftOutcome, String> {
    if !req.slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') || req.slug.is_empty() {
        return Err("slug: lowercase letters, digits and -".into());
    }
    let (guide, context) = context(req, registry)?;
    let dir = req.lessons.join("drafts").join(&req.slug);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("lesson.md");
    let mut previous: Option<(String, Vec<String>)> = None;
    let mut last = DraftOutcome { path: path.clone(), rounds: 0, findings: vec![], errors: 0 };
    for round in 1..=req.rounds.max(1) {
        let q = question(req, previous.as_ref().map(|(d, f)| (d.as_str(), f.as_slice())));
        let body = ask(&guide, &q, &context)?;
        // A reply wrapped in one outer fence is unwrapped; the lesson's own fences stay.
        let trimmed = body.trim();
        let text = match trimmed.strip_prefix("```").filter(|_| !trimmed.starts_with("```sim-")) {
            Some(rest) if trimmed.ends_with("```") && rest.lines().next().is_some_and(|l| l.trim().is_empty() || l.trim() == "markdown" || l.trim() == "md") => {
                let inner = rest.split_once('\n').map(|(_, r)| r).unwrap_or("");
                inner.trim_end().trim_end_matches("```").trim().to_string() + "\n"
            }
            _ => trimmed.to_string() + "\n",
        };
        sim_annotate::store::write_atomic(&path, text.as_bytes())?;
        let reports = lesson::check_path(&path, registry, CheckOptions::default());
        let findings: Vec<String> = reports.iter().flat_map(|r| r.findings.iter()).map(|f| f.to_string()).collect();
        let errors = reports.iter().flat_map(|r| &r.findings).filter(|f| f.severity == Severity::Error).count();
        last = DraftOutcome { path: path.clone(), rounds: round, findings: findings.clone(), errors };
        if errors == 0 {
            break;
        }
        previous = Some((text, findings));
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drafts_are_checked_revised_and_kept_in_drafts() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let lessons = std::env::temp_dir().join(format!("draft-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&lessons);
        std::fs::create_dir_all(&lessons).unwrap();
        std::fs::copy(root.join("lessons/AUTHORING_PROMPT.md"), lessons.join("AUTHORING_PROMPT.md")).unwrap();
        let system = root.join("lessons/motor-torque-speed/motor.system.json");
        let req = DraftRequest { slug: "motor-intro".into(), system: system.clone(), objectives: "say what sets a motor's torque".into(), lessons: lessons.clone(), rounds: 3 };
        let registry = crate::system_registry();
        let mut calls = 0;
        let outcome = draft(&req, &registry, |guide, q, ctx| {
            calls += 1;
            assert!(guide.contains("Pacing rules"), "the authoring guide is the instructions");
            let sys = ctx["system_path_for_front_matter"].as_str().unwrap().to_string();
            assert!(ctx["system"]["observables"].as_array().unwrap().iter().any(|o| o == "rotor.shaft.speed"));
            // First round: a claim that is wrong; second: fixed after seeing the finding.
            let bound = if q.contains("found problems") { "min: 700, max: 740" } else { "min: 900, max: 950" };
            Ok(format!("---\ntitle: Motor intro\nsystems: {{ motor: {sys} }}\n---\n# Motor\n\nText.\n\n```sim-scene\nid: s\nsystem: motor\nrun: {{ duration_s: 0.3, frame_rate: 100 }}\nset: {{ load.torque: -0.02 }}\nexpect:\n  - {{ observe: rotor.shaft.speed, reduce: mean, window: [0.25, 0.3], {bound} }}\n```\n"))
        }).unwrap();
        assert_eq!((calls, outcome.rounds, outcome.errors), (2, 2, 0), "{:?}", outcome.findings);
        assert!(outcome.path.starts_with(lessons.join("drafts")));
        let _ = std::fs::remove_dir_all(&lessons);
    }
}
