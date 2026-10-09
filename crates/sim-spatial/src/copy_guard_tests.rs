//! Copy guard (window-first-usability): window text names the window's own
//! controls, never REST or its payloads.
//!
//! The rule: every string literal in `src/**/*.rs` containing the word
//! "REST" (a whole word: "RESTORE" or "REST_X" do not count), or one of
//! [`PAYLOADS`] (`cad_state.`, `robot_state.`, `system_ui `: REST answers
//! and the automation surface), fails the test, naming `path:line` and the
//! literal, unless it is
//!
//! - a capability description: an argument of a `spec(…)` call, or of a
//!   `c(…)` call in one of [`C_SPEC_FILES`] (the files whose local `fn c`
//!   builds a `Spec`; elsewhere `c` is something else, e.g. the CAD command
//!   registry's window labels), at any depth over any number of lines; or in
//!   one of [`DESCRIPTION_FILES`] (the registries' description tables);
//! - in a test: files named `tests.rs` or `*_tests.rs`, files under a `tests` directory, and
//!   everything from an inline `#[cfg(test)]` `mod … {` to the end of a file
//!   (inline test modules come last);
//! - on [`ALLOWLIST`]: (file, a snippet of the literal, why it may say REST).
//!   An entry that no longer matches anything fails too, so the list stays
//!   exact.
//!
//! Comments (`//`, `///`, `//!`, `/* */`) are not literals and are never
//! read. Literals are found by a small lexer ([`literals`]) that knows
//! normal, raw (`r`, `br`, `cr`) and byte strings, escapes, char literals
//! and lifetimes, and tracks open parentheses to know whether a literal is
//! inside a `spec(` or (in [`C_SPEC_FILES`]) `c(` call.

/// The registries' description tables and the modes' agent guides
/// (`cad_guide`, `robot_guide`, `project_guide`, `inspect_guide`,
/// `lesson_guide`, `place_guide`, `phenomena_guide`: answered only over
/// REST), skipped whole (their text answers REST callers;
/// robot/actions/commands.rs also converts wire commands, whose errors go
/// back to the REST caller only).
const DESCRIPTION_FILES: [&str; 9] = ["cad/specs.rs", "robot/actions/commands.rs", "cad/guide.rs", "robot/guide.rs", "project/guide.rs", "inspect_guide.rs", "lesson/guide.rs", "place_guide.rs", "phenomena/guide.rs"];

/// (file under src/, snippet of the literal, reason).
const ALLOWLIST: &[(&str, &str, &str)] = &[
    ("builder/system_actions.rs", "Legacy read-only archive review compatibility API.", "capability description constructed inside the local Spec helper; never rendered window copy"),
    ("rest.rs", "Answering REST commands", "macOS activity reason (NSProcessInfo): seen by the OS, never shown in the window"),
    ("main.rs", "Physical REST (", "eprintln of the REST address to the terminal at launch, not window text"),
    ("robot/hardware/handlers.rs", "not accepted from REST or system_ui", "hardware safety refusal answered only to a REST or system_ui caller; stays as written"),
    ("robot/actions/mod.rs", "REST and system_ui may not start", "SYNC_REMOTE_REFUSAL: live motor sync refusal answered only to remote callers; stays as written"),
    ("cad/ops/catalogue/rest_only.rs", "a REST run must pass the revision", "the op's source note in the REST catalogue (where the revision rule comes from), read through cad_op's catalogue"),
    ("lesson/actions.rs", "REST edit", "origin label of a lesson_edit sent without a label (where the edit came from, as undo history shows it)"),
    ("lesson/actions.rs", "REST note", "origin label of a lesson note command (where the note came from)"),
    ("builder/system_actions.rs", "command(s) via REST", "origin label of a system batch sent without a label (where the edit came from, as undo history shows it)"),
    ("cad/surfaces/registry.rs", "REST API: show address", "RoboCAD's own command name (api.address shows RoboCAD's API address): a command label, not an instruction to use REST"),
    // Instructions sent to the AI agent (sim_agent), never drawn in the window.
    ("cad/threads/ai.rs", "You are the design assistant inside a CAD editor", "the CAD AI's developer instructions, sent to the agent"),
    ("cad/threads/ai.rs", "The viewer's REST API is at", "the CAD AI's turn instructions (how the agent reaches the viewer), sent to the agent"),
    ("project/chat.rs", "You are the robot design assistant", "the design assistant's developer instructions, sent to the agent"),
    ("project/chat.rs", "The viewer's REST API is at", "the design assistant's turn instructions (how the agent reaches the viewer), sent to the agent"),
    // Answers only a REST or system_ui caller receives (the window never shows them).
    ("robot/preset.rs", "robot_state.run.error", "OPENABLE_RULE: the openable_rule field of the preset listing JSON and the robot_presets description"),
    ("robot/graphs.rs", "system_ui graphs:toggle", "the toggle field of robot_state.graphs (REST JSON)"),
    ("cad/actions.rs", "available in cad_state.local_mass", "cad_physical's Ok message: a click's Ok outcome is dropped (actions::serve answers only REST with it)"),
    ("cad/actions.rs", "its outcome shows in cad_state.status", "wait_edit: only a REST caller waits on an edit (the edit continuation is set only when call.rest())"),
    ("cad/actions.rs", "see cad_state.status", "wait_edit: only a REST caller waits on an edit (the edit continuation is set only when call.rest())"),
    ("cad/ui_api.rs", "system_ui activate needs an id", "answer to a system_ui caller"),
    ("robot/actions/mod.rs", "{what} from REST", "origin label of a remote drive or run press (DriveInput::last_action: where the press came from)"),
    ("cad/ui_api.rs", "system_ui in CAD mode", "answer to a system_ui caller"),
    ("cad/files/jobs.rs", "its outcome shows in cad_state.files.last", "files::jobs::wait: a REST caller's job"),
    ("cad/files/jobs.rs", "see cad_state.files.last", "files::jobs::wait: a REST caller's job"),
    ("cad/ops/mod.rs", "see cad_state.ops", "unknown operation id: only a typed id (REST cad_run) can name one; the window's controls send catalogue ids"),
    ("cad/display/mod.rs", "cad_state.display.section.exact shows it", "cad_section's Ok message: a click's Ok outcome is dropped (actions::serve answers only REST with it)"),
    ("app/picker/mod.rs", "or with system_ui mode:<mode>", "picker::activate: answers a system_ui picker:* activation"),
    ("app/switch/mod.rs", "system_ui in {} mode has only", "switcher_ui: answer to a system_ui caller"),
    ("camera/mod.rs", "system_ui activate needs a camera:* control id", "answer to a system_ui caller"),
    ("phenomena/actions.rs", "system_ui activate needs an id", "answer to a system_ui caller"),
    ("phenomena/actions.rs", "system_ui in phenomena mode", "answer to a system_ui caller"),
];

/// Besides the word REST, what window text may not name: REST answers' paths
/// and the automation surface.
const PAYLOADS: [&str; 3] = ["cad_state.", "robot_state.", "system_ui "];

/// The files (under src/) whose local `fn c(…) -> Spec` builds capability
/// descriptions; elsewhere a `c(` call is not exempt.
const C_SPEC_FILES: [&str; 3] = ["robot/actions/commands.rs", "lesson/actions.rs", "builder/system_actions.rs"];

fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The text before an inline `#[cfg(test)]` `mod … {` (all of it without one).
pub(crate) fn strip_inline_tests(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != "#[cfg(test)]" {
            continue;
        }
        let next = lines[i + 1..].iter().map(|l| l.trim()).find(|l| !l.is_empty());
        if let Some(next) = next
            && (next.starts_with("mod ") || next.starts_with("pub mod ") || (next.starts_with("pub(") && next.contains(" mod ")))
            && next.ends_with('{')
        {
            return lines[..i].join("\n");
        }
    }
    text.to_string()
}

/// One string literal: its first line (1-based), its text as written
/// (escapes kept) and whether it is inside a `spec(` call (or a `c(` call
/// when `c_specs`).
#[derive(Debug, PartialEq)]
struct Literal {
    line: usize,
    text: String,
    in_spec: bool,
}

/// Every string literal of a Rust source (see the module doc); `c_specs`:
/// a `c(` call holds capability descriptions too.
fn literals(src: &str, c_specs: bool) -> Vec<Literal> {
    let c: Vec<char> = src.chars().collect();
    let n = c.len();
    let (mut i, mut line) = (0usize, 1usize);
    let mut out = Vec::new();
    let mut calls: Vec<bool> = Vec::new();
    let mut last = String::new();
    let starts = |i: usize, s: &str| s.chars().enumerate().all(|(k, x)| c.get(i + k) == Some(&x));
    while i < n {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if starts(i, "//") {
            while i < n && c[i] != '\n' {
                i += 1;
            }
            last.clear();
            continue;
        }
        if starts(i, "/*") {
            let mut depth = 1;
            i += 2;
            while i < n && depth > 0 {
                if starts(i, "/*") {
                    depth += 1;
                    i += 2;
                } else if starts(i, "*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    if c[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            last.clear();
            continue;
        }
        // A raw string: r"…", r#"…"#, br"…", br#"…"#, cr#"…"#, … (not an
        // identifier ending in r; an identifier `br` or `cr` is read below).
        let prefix = match (ch, c.get(i + 1).copied()) {
            ('r', _) => 1,
            ('b' | 'c', Some('r')) => 2,
            _ => 0,
        };
        if prefix > 0 && (i == 0 || !ident(c[i - 1])) {
            let mut j = i + prefix;
            let mut hashes = 0;
            while j < n && c[j] == '#' {
                hashes += 1;
                j += 1;
            }
            if j < n && c[j] == '"' {
                let close = format!("\"{}", "#".repeat(hashes));
                let start = line;
                let mut text = String::new();
                j += 1;
                while j < n && !starts(j, close.as_str()) {
                    if c[j] == '\n' {
                        line += 1;
                    }
                    text.push(c[j]);
                    j += 1;
                }
                out.push(Literal { line: start, text, in_spec: calls.iter().any(|s| *s) });
                i = j + close.chars().count();
                last.clear();
                continue;
            }
        }
        if ch == '"' {
            let start = line;
            let mut text = String::new();
            let mut j = i + 1;
            while j < n && c[j] != '"' {
                if c[j] == '\\' && j + 1 < n {
                    text.push(c[j]);
                    text.push(c[j + 1]);
                    if c[j + 1] == '\n' {
                        line += 1;
                    }
                    j += 2;
                    continue;
                }
                if c[j] == '\n' {
                    line += 1;
                }
                text.push(c[j]);
                j += 1;
            }
            out.push(Literal { line: start, text, in_spec: calls.iter().any(|s| *s) });
            i = j + 1;
            last.clear();
            continue;
        }
        if ch == '\'' {
            // A char literal ('x', '\n', '\u{…}') or a lifetime ('a).
            if i + 1 < n && c[i + 1] == '\\' {
                // Past the backslash and the escaped character ('\'' too).
                let mut j = i + 3;
                while j < n && c[j] != '\'' {
                    j += 1;
                }
                i = j + 1;
            } else if i + 2 < n && c[i + 2] == '\'' {
                i += 3;
            } else {
                i += 1;
            }
            last.clear();
            continue;
        }
        if ident(ch) {
            let mut j = i;
            while j < n && ident(c[j]) {
                j += 1;
            }
            last = c[i..j].iter().collect();
            i = j;
            continue;
        }
        match ch {
            ' ' | '\t' | '\r' => {}
            '(' => {
                calls.push(last == "spec" || (c_specs && last == "c"));
                last.clear();
            }
            ')' => {
                calls.pop();
                last.clear();
            }
            _ => last.clear(),
        }
        i += 1;
    }
    out
}

/// Whether `text` sends a person to REST: the whole word "REST" or one of [`PAYLOADS`].
fn names_rest(text: &str) -> bool {
    says_rest(text) || PAYLOADS.iter().any(|p| text.contains(p))
}

/// Whether `text` has "REST" as a whole word.
fn says_rest(text: &str) -> bool {
    let c: Vec<char> = text.chars().collect();
    let w: Vec<char> = "REST".chars().collect();
    (0..c.len()).any(|i| c[i..].starts_with(&w) && (i == 0 || !ident(c[i - 1])) && c.get(i + w.len()).is_none_or(|x| !ident(*x)))
}

#[test]
fn window_text_does_not_send_people_to_rest() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut used = vec![false; ALLOWLIST.len()];
    let mut dirs = vec![src.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_none_or(|n| n != "tests") {
                    dirs.push(path);
                }
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let relative = path.strip_prefix(&src).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let name = relative.rsplit('/').next().unwrap_or("");
            // The registries' description tables only: other files named
            // specs.rs (cad/sketch/specs.rs is sketch logic whose errors
            // reach the status line) are window text like any other.
            if name == "tests.rs" || name.ends_with("_tests.rs") || DESCRIPTION_FILES.contains(&relative.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let c_specs = C_SPEC_FILES.contains(&relative.as_str());
            for literal in literals(&strip_inline_tests(&text), c_specs) {
                if literal.in_spec || !names_rest(&literal.text) {
                    continue;
                }
                let allowed: Vec<usize> = ALLOWLIST.iter().enumerate().filter(|(_, (file, snippet, _))| *file == relative && literal.text.contains(snippet)).map(|(k, _)| k).collect();
                if allowed.is_empty() {
                    offenders.push(format!("{relative}:{}: \"{}\"", literal.line, literal.text.chars().take(160).collect::<String>()));
                }
                for k in allowed {
                    used[k] = true;
                }
            }
        }
    }
    let stale: Vec<String> = ALLOWLIST.iter().zip(&used).filter(|(_, u)| !**u).map(|((file, snippet, _), _)| format!("{file}: {snippet:?}")).collect();
    assert!(
        offenders.is_empty(),
        "window text sends people to REST (or names cad_state., robot_state. or system_ui); name the window's control (or add the missing control) instead, or allowlist it with a reason in copy_guard_tests.rs:\n{}",
        offenders.join("\n")
    );
    assert!(stale.is_empty(), "copy guard allowlist entries that match no literal any more (remove them):\n{}", stale.join("\n"));
}

#[test]
fn the_lexer_finds_literals_and_spec_calls() {
    let src = "// \"REST in a comment\"\nlet a = \"use REST x\";\nspec(\n  \"name\",\n  format!(\"REST {}\", 1),\n);\nlet b = r#\"raw \"REST\" here\"#; let q = '\"'; fn f<'a>() {}\n/* \"REST\" */ let z = \"RESTORE\";";
    let found = literals(src, true);
    let texts: Vec<(usize, &str, bool)> = found.iter().map(|l| (l.line, l.text.as_str(), l.in_spec)).collect();
    assert_eq!(texts, vec![(2, "use REST x", false), (4, "name", true), (5, "REST {}", true), (7, "raw \"REST\" here", false), (8, "RESTORE", false)]);
    assert!(says_rest("use REST x") && says_rest("REST") && says_rest("(REST cad_open)"));
    assert!(!says_rest("RESTORE") && !says_rest("REST_X") && !says_rest("rest") && !says_rest("XREST"));
    assert!(names_rest("see cad_state.status") && names_rest("robot_state.run.error") && names_rest("system_ui activate") && !names_rest("cad_state") && !names_rest("the robot state"));
    // `c(` holds descriptions only where asked (a CAD command label elsewhere).
    let labels: Vec<bool> = literals("c(\"REST API\", c(\"x\"))", false).iter().map(|l| l.in_spec).collect();
    assert_eq!(labels, vec![false, false]);
    let specs: Vec<bool> = literals("c(\"REST API\", spec(\"x\"))", false).iter().map(|l| l.in_spec).collect();
    assert_eq!(specs, vec![false, true]);
    // Byte and C raw strings, quotes inside; `br`/`cr` as identifiers are not prefixes.
    let raw = "let a = br#\"x \"REST\" y\"#; let b = br\"z\"; let c = cr#\"w\"#; let br = 1; f(br, \"v\");";
    let texts: Vec<String> = literals(raw, false).into_iter().map(|l| l.text).collect();
    assert_eq!(texts, vec!["x \"REST\" y", "z", "w", "v"]);
    assert_eq!(strip_inline_tests("a\n#[cfg(test)]\nmod tests;\nb"), "a\n#[cfg(test)]\nmod tests;\nb");
    assert_eq!(strip_inline_tests("a\n#[cfg(test)]\nmod tests {\n\"REST\"\n}"), "a");
}
