"""Muse (Meta's `muse exec`) as a coordinator backend.

Probed against Muse Code 1.4.6 on 2026-10-12 (isolated temp repos):

- `muse exec --provider meta --json` streams session-log envelopes on
  stdout. The answer is the `run_terminal` payload's text; with
  `--output-schema FILE` (Meta providers only) that text is JSON matching
  the schema, so the coordinator shims it into its shared result shape:
  `structured_output`, `result`, `subtype`, `session_id`.
- The prompt does not come from stdin: exec wants an argument or
  `--prompt-file`, so the coordinator passes the log's `.prompt.md` file.
- Sessions are coordinator-chosen like Claude's: `--session-id <uuid>`
  creates the session on first use and resumes it afterwards (verified:
  a second call recalled the first call's file edit). A session id needs
  retained session logging, so never pass `--no-session-log` for one.
- There is no system-prompt or developer-instructions flag (`-c` rejects
  `system_prompt`/`instructions`/`developer_instructions`), so role
  standing instructions ride in the prompt on a session's first turn,
  like Codex's first turn, and again when they change.
- A session's tool posture is fixed at creation: a session created with
  `--disable-write` stayed unable to edit when resumed without it. The
  coordinator therefore retires a Muse session when the run's
  audit-only posture changes (`posture()`).
- The exec stream carries no token or dollar accounting (the session
  log's model records report zeroed usage), so cost settles at 0 like
  Codex's; `max_turns` maps to `--max-model-steps`, and there is no
  fast-mode or per-window usage signal to auto-switch on.
"""
import hashlib
import json
from pathlib import Path

NAME = "Muse"


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def fresh_prompt(instructions, prompt):
    return ("STANDING INSTRUCTIONS for your role in this run. They and the task below are your whole brief; "
            "they persist for this session and are not repeated on later turns.\n\n" + instructions +
            "\n\nTHE TASK\n\n" + prompt)


def updated_instructions(instructions):
    return ("INSTRUCTION UPDATE: the standing instructions you are following for this run have changed. "
            "The text below replaces the previous version in full; follow it from here on.\n\n" + instructions +
            "\n\nTHE TASK\n\n")


def role_note(capped_seconds=None):
    note = ("You are running under Muse (`muse exec`) driven by the claude-pair coordinator. "
            "Its process deadline still applies: if it stops your turn, the coordinator resumes this session, "
            "so leave the folder and your account of it resumable.")
    if capped_seconds:
        note += (f" Muse has no per-shell-command cap the coordinator can set, so keep each shell command "
                 f"inside {capped_seconds} seconds of foreground time yourself (prefix cargo and other slow "
                 f"commands with `within {capped_seconds}`), and run one cargo command at a time. A command "
                 f"that runs long in the background stays yours to watch and finish within the turn.")
    return note


#: pair subagent tool names (Claude vocabulary) -> Muse tool ids.
TOOLS = {"Read": "read_file", "Grep": "search", "Glob": "search", "Edit": "edit_file",
         "Write": "write_file", "Bash": "bash", "WebFetch": "web_fetch", "WebSearch": "web_search"}


def agent_overlay(path):
    """The project subagent definitions as exec's ephemeral Agent
    Definition overlay (`--agents {"agents": [...]}`): the same file the
    Claude and Codex backends translate, tool names re-vocabularised."""
    try:
        raw = json.loads(Path(path).read_text())
    except (OSError, json.JSONDecodeError):
        return None
    definitions = []
    for name, entry in raw.items():
        tools = []
        for tool in entry.get("tools") or []:
            mapped = TOOLS.get(tool)
            if mapped and mapped not in tools:
                tools.append(mapped)
        definitions.append({"name": name, "description": entry.get("description") or name,
                            "prompt": entry.get("prompt") or "", "tools": tools})
    return json.dumps({"agents": definitions}, separators=(",", ":")) if definitions else None


def posture(audit_only):
    return "audit" if audit_only else "work"


def command(executable, *, workspace, session, schema_path, prompt_path, audit_only=False,
            model=None, effort=None, max_turns=None, agents=None):
    """argv for one Muse turn, resuming `session` (coordinator-chosen, like Claude).

    Meta provider (the only one honouring `--output-schema`), JSON-lines
    stream, prompt from file, reminders off and no foreign personal
    context: the coordinator's prompt is the whole brief. Writes follow
    the posture the session was created with (see the module notes):
    audit sessions keep Muse's sandbox and lose the dedicated edit
    tools, work sessions run with `--yolo` like the other backends run
    without permission prompts. Exec has no extra-directory flag: the
    run state directory lives inside the workspace, so it is already in
    reach (an out-of-tree `--state` is only reachable unsandboxed).
    """
    argv = [executable, "exec", "--provider", "meta", "--workspace", str(workspace), "--json",
            "--prompt-file", str(prompt_path), "--output-schema", str(schema_path),
            "--session-id", session, "--trust-workspace", "--disable-reminders",
            "--no-foreign-personal-context"]
    if audit_only:
        argv += ["--disable-write", "--approval-mode", "never"]
    else:
        argv += ["--yolo"]
    if model:
        argv += ["--model", model]
    if effort:
        argv += ["--effort", effort]
    if max_turns:
        argv += ["--max-model-steps", str(max_turns)]
    if agents:
        argv += ["--agents", agents]
    return argv


def records(raw):
    """The JSON values of an exec JSON-lines stream (session-log envelopes)."""
    out = []
    for line in raw.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return out


def payload(record):
    value = record.get("payload")
    return value if isinstance(value, dict) else record


def session_id(raw):
    for record in records(raw):
        stream = record.get("stream") or {}
        if stream.get("kind") == "session" and stream.get("id"):
            return stream["id"]
    return None


def started(raw):
    return any(payload(r).get("kind") in ("run_started", "turn_input_user") for r in records(raw))


def _usage(recs):
    """Token totals if a stream ever carries them (none do today)."""
    usage = {}
    keys = ("input_tokens", "output_tokens", "cached_tokens", "reasoning_tokens")

    def visit(value):
        if isinstance(value, dict):
            if any(k in value for k in keys):
                for k in keys:
                    if isinstance(value.get(k), (int, float)):
                        usage[k] = usage.get(k, 0) + value[k]
            for item in value.values():
                visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)
    for record in recs:
        visit(payload(record))
    return usage


def parse(raw):
    """The coordinator's shared result shape from an exec stream, or None."""
    recs = records(raw)
    terminal = None
    model = None
    sid = None
    denials = []
    for record in recs:
        stream = record.get("stream") or {}
        if stream.get("kind") == "session" and stream.get("id"):
            sid = stream["id"]
        body = payload(record)
        kind = body.get("kind")
        if kind == "run_terminal":
            terminal = body
        elif kind == "run_model_configured":
            model = body.get("model_id") or model
        elif kind == "tool_result":
            facts = body.get("correlation_facts") or {}
            outcome = facts.get("outcome")
            if outcome not in (None, "success"):
                denials.append({"tool_name": facts.get("tool_name"), "outcome": outcome,
                                "text": str(body.get("text") or "")[:600]})
    if terminal is None:
        return None
    text = terminal.get("text") or ""
    data = None
    try:
        parsed = json.loads(text)
        if parsed is not None:
            data = parsed
    except (json.JSONDecodeError, TypeError):
        pass
    done = terminal.get("terminal") == "completed"
    return {"type": "result", "subtype": "success" if done else str(terminal.get("terminal") or "error"),
            "is_error": not done, "result": text or str(terminal.get("reason") or ""),
            "reason": terminal.get("reason"), "structured_output": data, "session_id": sid,
            "usage": _usage(recs), "model": model, "permission_denials": denials}


def _clip_lines(text, lines=30, chars=3000):
    text = str(text or "")
    kept = text.splitlines()[:lines]
    return "\n".join(kept)[:chars]


def activity(raw):
    """Dashboard rows: model text (deltas coalesced), tools with outcomes."""
    rows = []
    message = None

    def flush():
        nonlocal message
        if message is not None:
            rows.append({"kind": "message", "text": message["text"][:6000], "at": message["at"]})
            message = None

    def at(record):
        stamp = record.get("recorded_at")
        return stamp / 1_000_000 if isinstance(stamp, (int, float)) and stamp > 1e14 else stamp

    for record in records(raw):
        body = payload(record)
        kind = body.get("kind")
        when = at(record)
        if kind == "run_model_configured":
            flush()
            rows.append({"kind": "session", "text": "Session started · " + str(body.get("display_label") or body.get("model_id") or "Muse"), "at": when})
        elif kind == "run_output_delta":
            if message is None:
                message = {"text": "", "at": when}
            message["text"] += str(body.get("text") or "")
        elif kind == "tool_result":
            flush()
            facts = body.get("correlation_facts") or {}
            outcome = facts.get("outcome") or "done"
            rows.append({"kind": "tool", "text": str(facts.get("tool_name") or "tool"), "at": when,
                         "id": body.get("call_id"), "name": facts.get("tool_name") or "tool",
                         "details": {"outcome": outcome, **(body.get("edit_facts") or {})},
                         "status": "done" if outcome == "success" else "error", "ended_at": when,
                         "output": _clip_lines(body.get("text"))})
    flush()
    result = parse(raw)
    return rows, result
