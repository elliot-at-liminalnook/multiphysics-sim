"""Codex CLI as a pair backend, behind the same call contract as Claude Code.

The coordinator speaks Claude Code's result shape; this module turns a
`codex exec --json` turn into that shape, so the ledger, retries, usage-limit
waits, notebook and dashboard work unchanged. Standard library only.

Verified against codex-cli 0.157.1 on 2026-10-01 (probes in the pair README,
"Codex backend"):
- `codex exec --json` prints JSONL: `thread.started` (thread_id), `turn.started`,
  `item.started` / `item.completed` (agent_message, command_execution,
  file_change, mcp_tool_call, web_search, todo_list, reasoning, error),
  `turn.completed` (usage), `turn.failed`, `error`. No timestamps, no cost.
- `--output-schema` makes the final agent_message a JSON document.
- Piped stdin is appended to the prompt and read to EOF, so the prompt goes in
  on stdin with `-` and stdin is closed.
- `developer_instructions` is fixed when a session starts: `exec resume`
  ignores a new value, so changed instructions are restated in the prompt.
- Custom subagent roles: `-c agents.<name>.description=...` and
  `-c agents.<name>.config_file=<toml>` (names with hyphens are rejected;
  project `.codex/agents/` files were not loaded for an untrusted folder).
  Subagents run in parallel, do not inherit the parent's developer
  instructions, and write their own rollout files whose session_meta names
  the parent thread. Their briefs are encrypted in both rollouts.
- Rate limits are not in the stream; each rollout's `token_count` events
  carry `rate_limits` (primary/secondary windows with used_percent,
  window_minutes, resets_at; rate_limit_reached_type when hit).
- `service_tier = "priority"` is fast mode; a model that doesn't offer it
  emits an error item and the request runs without it.
- There is no per-command shell timeout setting, so `shims/cargo` keeps cargo
  inside the run's cap.
"""
import datetime
import glob
import hashlib
import json
import os
from pathlib import Path
import re

NAME = "codex"
HERE = Path(__file__).resolve().parent

# Built-in features a run never needs. Like Claude Code's --no-chrome and empty
# MCP config, they keep the user's personal setup out of the run. Set through
# -c, so a feature a later Codex removes is ignored instead of refused.
DISABLED_FEATURES = ("memories", "browser_use", "browser_use_external", "computer_use", "in_app_browser", "apps",
                     # A session snapshots its shell environment when it starts and a
                     # resumed session reuses it, so PATH (the cargo queue and time
                     # limit) and PAIR_* changes would never reach it.
                     "shell_snapshot")

ROLE_NOTE = """# This backend: Codex CLI

These instructions were written for Claude Code. In this run they mean:
- **Subagents.** The `Agent` tool is `spawn_agent`. The agent types are
  `pair_implementer` (pair-implementer), `pair_reviewer` (pair-reviewer) and
  `explorer` (Explore). Spawn several in the same step to run them in
  parallel, then `wait_agent` until every one has reported. Wherever these
  instructions or the assignment say to fan out or split work across
  subagents, that is an explicit request for parallel agent work. A subagent
  starts without your instructions, so give it a complete brief.
- **Shell time.** Codex does not stop shell commands for you.{shell}
- **Your result** is your final message: one JSON document matching the
  schema, with nothing after it.
"""
SHELL_CAPPED = (" Run anything that builds or runs code as `within {seconds} <command>`."
                " `cargo` on your PATH already runs inside that limit and reports a timeout"
                " as unverified. Where these instructions say Claude Code stops every shell"
                " command after 10 seconds, this is how that holds here.")
SHELL_FREE = " This turn has no shell time limit."

SUBAGENT_NOTE = """Codex note: Codex does not stop shell commands for you. Run anything that
builds or runs code as `within 10 <command>`; `cargo` on your PATH already runs
inside that limit. Where this brief says Claude Code stops commands after 10
seconds, this is how that holds here."""


def toml_string(text):
    """A TOML basic string. JSON's escapes are valid TOML; keeping non-ASCII
    raw avoids the surrogate-pair escapes TOML forbids."""
    return json.dumps(text, ensure_ascii=False)


def agent_name(name):
    """Codex rejects hyphenated role names; pair-implementer -> pair_implementer."""
    return re.sub(r"[^A-Za-z0-9_]", "_", name)


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def home():
    return Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex")


def role_note(shell_seconds):
    shell = SHELL_CAPPED.format(seconds=f"{shell_seconds:g}") if shell_seconds else SHELL_FREE
    return ROLE_NOTE.format(shell=shell)


def agent_args(root, subagents_path):
    """`-c` overrides that register the run's subagent roles for one call, from
    the same subagents.json Claude Code receives. The role files live in the run
    state, so nothing is written to the user's ~/.codex or the repository."""
    if not subagents_path.exists():
        return []
    out = root / "codex-agents"
    out.mkdir(exist_ok=True)
    args = []
    for name, spec in json.loads(subagents_path.read_text()).items():
        role = agent_name(name)
        lines = [f"developer_instructions = {toml_string(spec['prompt'] + chr(10) + chr(10) + SUBAGENT_NOTE)}"]
        if not {"Edit", "Write", "MultiEdit"} & set(spec.get("tools", [])):
            lines.append('sandbox_mode = "read-only"')  # a reviewer reads; it never edits
        path = out / f"{role}.toml"
        text = "\n".join(lines) + "\n"
        if not path.exists() or path.read_text() != text:
            path.write_text(text)
        args += ["-c", f"agents.{role}.description={toml_string(spec['description'])}",
                 "-c", f"agents.{role}.config_file={toml_string(str(path))}"]
    return args


def command(executable, *, session, schema_path, instructions, fast, model, effort, audit_only, agents):
    """argv for one turn. The prompt goes on stdin (`-`)."""
    argv = [executable, "exec"] + (["resume"] if session else [])
    # Like Claude Code's --setting-sources project: the user's personal
    # config.toml (plugins, MCP servers, model, memories) stays out of the run.
    argv += ["--json", "--ignore-user-config", "--output-schema", str(schema_path)]
    if audit_only:
        argv += ["-c", 'sandbox_mode="read-only"', "-c", 'approval_policy="never"', "-c", "features.multi_agent=false"]
    else:
        argv += ["--dangerously-bypass-approvals-and-sandbox"]
    # A non-login shell keeps the coordinator's PATH (within, the cargo cap) in front.
    argv += ["-c", "allow_login_shell=false"]
    for feature in DISABLED_FEATURES:
        argv += ["-c", f"features.{feature}=false"]
    if not session:
        argv += ["-c", "developer_instructions=" + toml_string(instructions)]
    if not audit_only:
        argv += agents
    if fast:
        argv += ["-c", 'service_tier="priority"']
    if model:
        argv += ["-m", model]
    if effort:
        argv += ["-c", f"model_reasoning_effort={toml_string(effort)}"]
    if session:
        argv.append(session)
    return argv + ["-"]


def updated_instructions(instructions):
    """Prepended to a resumed turn whose role instructions changed: Codex keeps
    the developer instructions a session started with."""
    return ("STANDING INSTRUCTIONS UPDATED. The coordinator's instructions for your role changed since "
            "this session started. The text below replaces your earlier developer instructions in full; "
            "where they differ, follow this version.\n\n" + instructions + "\n\n---\n\n")


def stream(raw):
    for line in raw.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict):
            yield event


def thread_id(raw):
    return next((e.get("thread_id") for e in stream(raw) if e.get("type") == "thread.started"), None)


WORK_ITEMS = {"agent_message", "command_execution", "file_change", "mcp_tool_call", "web_search", "todo_list", "reasoning"}


def started(raw):
    """True once the model did anything (the Claude path's "an assistant event")."""
    return any(e.get("type", "").startswith("item.") and (e.get("item") or {}).get("type") in WORK_ITEMS for e in stream(raw))


def error_text(value):
    """Codex nests the API error as JSON text inside `message`."""
    text = value.get("message", "") if isinstance(value, dict) else str(value or "")
    try:
        inner = json.loads(text)
        return (inner.get("error") or {}).get("message") or text
    except (json.JSONDecodeError, AttributeError):
        return text


def parse(raw, fast_requested):
    """One turn's stdout as Claude Code's result shape, or None when the output
    ended without a turn outcome (a crash or a cut-off)."""
    tid, messages, errors, notices, usage, done, failed = None, [], [], [], None, False, None
    for e in stream(raw):
        kind = e.get("type")
        item = e.get("item") or {}
        if kind == "thread.started":
            tid = e.get("thread_id")
        elif kind == "item.completed" and item.get("type") == "agent_message":
            messages.append(item.get("text", ""))
        elif kind == "item.completed" and item.get("type") == "error":
            notices.append(item.get("message", ""))
        elif kind == "error":
            errors.append(error_text(e))
        elif kind == "turn.completed":
            done, usage = True, e.get("usage")
        elif kind == "turn.failed":
            failed = error_text(e.get("error"))
    if not done and failed is None:
        return None
    result = {"type": "result", "backend": NAME, "session_id": tid, "total_cost_usd": None, "usage": usage,
              "permission_denials": [], "structured_output": None}
    if fast_requested:
        refused = next((n for n in notices if "service tier" in n.lower()), None)
        result.update(fast_mode_state="off" if refused else "on", fast_mode_disabled_reason=refused)
    if failed is not None:
        return {**result, "subtype": "error", "is_error": True, "result": failed or "; ".join(errors) or "turn failed"}
    final = messages[-1] if messages else ""
    try:
        result["structured_output"] = json.loads(final)
    except json.JSONDecodeError:
        pass  # the coordinator's schema check reports it and retries the same session
    return {**result, "subtype": "success", "is_error": False, "result": final}


def rollout(tid):
    """The session's rollout file under CODEX_HOME/sessions/YYYY/MM/DD."""
    if not tid:
        return None
    found = sorted(glob.glob(str(home() / "sessions" / "*" / "*" / "*" / f"rollout-*-{tid}.jsonl")))
    return Path(found[-1]) if found else None


def rows(path, limit=None):
    try:
        with path.open("rb") as f:
            if limit and path.stat().st_size > limit:
                f.seek(path.stat().st_size - limit)
                f.readline()  # drop the partial first line
            data = f.read().decode(errors="replace")
    except OSError:
        return []
    out = []
    for line in data.splitlines():
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return out


WINDOWS = ((360, "five_hour"), (10080, "seven_day"))


def rate_limits(tid):
    """The newest rate-limit reading in the session's rollout, in the shape of
    Claude Code's rate_limit_event, so the usage-limit wait and the dashboard
    meters read either backend."""
    path = rollout(tid)
    reading = None
    for row in rows(path, 2_000_000) if path else []:
        payload = row.get("payload") or {}
        if payload.get("type") == "token_count" and isinstance(payload.get("rate_limits"), dict):
            reading = payload["rate_limits"]
    if not reading:
        return None
    windows, reached = {}, []
    for name in ("primary", "secondary"):
        w = reading.get(name)
        if not isinstance(w, dict):
            continue
        minutes = w.get("window_minutes") or 0
        key = next((k for limit, k in WINDOWS if minutes <= limit), f"{minutes}_minutes")
        used = (w.get("used_percent") or 0) / 100
        windows[key] = {"utilization": used, "resetsAt": w.get("resets_at"), "window_minutes": minutes}
        if used >= 1:
            reached.append((key, w.get("resets_at")))
    hit = bool(reading.get("rate_limit_reached_type")) or bool(reached)
    info = {"backend": NAME, "status": "rejected" if hit else "allowed", "unifiedWindows": windows,
            "plan_type": reading.get("plan_type"), "credits": reading.get("credits")}
    if reached:
        key, resets = max(reached, key=lambda r: r[1] or 0)
        info.update(rateLimitType=key, resetsAt=resets)
    elif hit:
        info["rateLimitType"] = str(reading.get("rate_limit_reached_type"))
    return info


# ---- Dashboard activity -------------------------------------------------------

def stamp(row):
    try:
        return datetime.datetime.fromisoformat(row["timestamp"].replace("Z", "+00:00")).timestamp()
    except (KeyError, ValueError, AttributeError):
        return None


def clip(text, chars=3000):
    text = str(text or "")
    return text if len(text) <= chars else text[:chars] + "\n…"


def shell_command(command):
    """['/bin/zsh', '-c', 'cmd'] or "/bin/zsh -c 'cmd'" -> cmd."""
    if isinstance(command, list):
        return command[-1] if len(command) >= 3 and command[-2] in ("-c", "-lc") else " ".join(map(str, command))
    match = re.match(r"^/bin/\w+ -l?c '(.*)'$", str(command or ""), re.S)
    return match.group(1) if match else str(command or "")


def diff_edits(diff):
    old = [l[1:] for l in diff.splitlines() if l.startswith("-") and not l.startswith("---")]
    new = [l[1:] for l in diff.splitlines() if l.startswith("+") and not l.startswith("+++")]
    return {"old": clip("\n".join(old[:40]), 4000), "new": clip("\n".join(new[:40]), 4000)}


def rollout_item(item, at, parent):
    """A rollout `item_completed` as a dashboard activity entry (or None)."""
    kind = item.get("type")
    base = {"at": at, "ended_at": at, "parent": parent, "id": item.get("id")}
    if kind == "AgentMessage":
        text = "".join(c.get("text", "") for c in item.get("content", []) if isinstance(c, dict))
        return {**base, "kind": "message", "text": text[:6000]} if text.strip() else None
    if kind == "CommandExecution":
        cmd = shell_command(item.get("command"))
        code = item.get("exit_code")
        return {**base, "kind": "tool", "name": "Bash", "text": "Bash: " + cmd[:350], "details": {"command": cmd[:4000]},
                "status": "error" if (code not in (None, 0) or item.get("status") == "failed") else "done",
                "output": clip(item.get("aggregated_output") or item.get("stdout"), 3000)}
    if kind == "FileChange":
        changes = item.get("changes") or {}
        files = list(changes) if isinstance(changes, dict) else []
        first = changes.get(files[0], {}) if files else {}
        return {**base, "kind": "tool", "name": "Edit", "text": "Edit: " + ", ".join(files)[:350],
                "details": {"file": ", ".join(files), "edits": [diff_edits(first.get("unified_diff", ""))] if first else []},
                "status": "error" if item.get("status") == "failed" else "done", "output": clip(item.get("stderr"), 1500)}
    if kind == "McpToolCall":
        return {**base, "kind": "tool", "name": "MCP", "text": f"MCP: {item.get('server')}.{item.get('tool')}",
                "details": {"server": item.get("server"), "tool": item.get("tool"), "arguments": clip(item.get("arguments"), 600)},
                "status": "error" if item.get("status") == "failed" else "done", "output": clip(item.get("result"), 1500)}
    if kind == "WebSearch":
        return {**base, "kind": "tool", "name": "WebSearch", "text": "WebSearch: " + str(item.get("query", ""))[:300],
                "details": {"query": item.get("query")}, "status": "done"}
    return None  # reasoning stays private; user messages and compaction aren't activity


def within(at, since, until):
    """A rollout row belongs to a call when it was written during it. A resumed
    session's rollout holds every earlier turn too."""
    return at is not None and (since is None or at >= since) and (until is None or at < until)


def spawned_by(meta):
    """A session's spawn record: Codex writes `source` as an object for a subagent
    and as a plain string ("exec", "vscode") for a top-level session."""
    source = meta.get("source") if isinstance(meta, dict) else None
    subagent = source.get("subagent") if isinstance(source, dict) else None
    spawn = subagent.get("thread_spawn") if isinstance(subagent, dict) else None
    return spawn if isinstance(spawn, dict) else {}


def lane(path, parent_tid, since=None, until=None):
    """A subagent's rollout as its spawn entry plus its activity during this call
    (None when it did nothing in it)."""
    data = rows(path, 4_000_000)
    meta = next((r.get("payload", {}) for r in data if r.get("type") == "session_meta"), {})
    tid = meta.get("id")
    spawn_meta = spawned_by(meta)
    if spawn_meta.get("parent_thread_id") != parent_tid:
        return None, []
    activity, final, ended, first, busy = [], None, None, None, False
    for row in data:
        payload = row.get("payload") or {}
        at = stamp(row)
        if row.get("type") != "event_msg" or not within(at, since, until):
            continue
        first = first or at
        if payload.get("type") == "item_completed":
            entry = rollout_item(payload.get("item") or {}, at, tid)
            if entry:
                activity.append(entry)
        elif payload.get("type") == "task_started":
            busy = True  # a new task, including a follow-up after an earlier one finished
        elif payload.get("type") == "task_complete":
            final, ended, busy = payload.get("last_agent_message"), at, False
    if first is None:
        return None, []
    name = spawn_meta.get("agent_path", "").rsplit("/", 1)[-1] or meta.get("agent_nickname") or "subagent"
    spawn = {"kind": "tool", "name": "Agent", "id": tid, "at": first, "ended_at": None if busy else ended, "parent": None,
             "text": f"Agent: {name}", "status": "done" if ended and not busy else "running", "output": clip(final, 3000),
             "details": {"subagent_type": spawn_meta.get("agent_role") or meta.get("agent_role") or "default",
                         "description": f"{name} ({meta.get('agent_nickname') or 'unnamed'})",
                         "prompt": "Codex encrypts subagent briefs, so the brief isn't shown. Its actions and final reply are below."},
             "total_actions": len(activity)}
    return spawn, activity


def children(parent_path, parent_tid):
    """Rollouts of the subagents a session spawned: same or following day."""
    day = parent_path.parent
    try:
        when = datetime.date(int(day.parent.parent.name), int(day.parent.name), int(day.name))
    except ValueError:
        return []
    found = []
    for offset in (0, 1):
        d = when + datetime.timedelta(days=offset)
        folder = home() / "sessions" / f"{d.year:04d}" / f"{d.month:02d}" / f"{d.day:02d}"
        for path in folder.glob("rollout-*.jsonl"):
            if path == parent_path:
                continue
            try:
                with path.open() as f:
                    head = f.readline()
            except OSError:
                continue
            if parent_tid not in head:
                continue  # cheap filter; the parsed spawn record decides
            try:
                meta = json.loads(head).get("payload")
            except (json.JSONDecodeError, AttributeError):
                continue
            if spawned_by(meta).get("parent_thread_id") == parent_tid:
                found.append(path)
    return sorted(found)


def activity(raw, tid=None, since=None, until=None):
    """Dashboard activity and a Claude-shaped result for one Codex call's stdout.
    Timed entries come from the session's rollout (the stream has no times),
    limited to rows written between `since` and `until` (this call's start and
    the next call's), because a resumed session's rollout holds every turn. A
    command still running comes from the stream. `tid` comes from the file's
    first line when `raw` is only its tail."""
    tid = tid or thread_id(raw)
    path = rollout(tid)
    items = []
    if path:
        for row in rows(path, 4_000_000):
            payload = row.get("payload") or {}
            at = stamp(row)
            if not within(at, since, until):
                continue
            if row.get("type") == "event_msg" and payload.get("type") == "item_completed":
                entry = rollout_item(payload.get("item") or {}, at, None)
                if entry:
                    items.append(entry)
            elif row.get("type") == "session_meta" and not items:
                items.append({"kind": "session", "text": "Session started · Codex " + str((row.get("payload") or {}).get("cli_version", "")),
                              "at": at})
        for child in children(path, tid):
            spawn, acts = lane(child, tid, since, until)
            if spawn:
                items.append(spawn)
                items.extend(acts)
    else:
        for e in stream(raw):  # no rollout yet (or CODEX_HOME moved): untimed stream items
            item = e.get("item") or {}
            if e.get("type") == "item.completed" and item.get("type") == "agent_message":
                items.append({"kind": "message", "text": item.get("text", "")[:6000], "at": None, "parent": None})
            elif e.get("type") == "item.completed" and item.get("type") == "command_execution":
                cmd = shell_command(item.get("command"))
                items.append({"kind": "tool", "name": "Bash", "text": "Bash: " + cmd[:350], "details": {"command": cmd},
                              "status": "done" if item.get("exit_code") == 0 else "error", "at": None, "parent": None,
                              "id": item.get("id"), "output": clip(item.get("aggregated_output"))})
    # A command started and not yet completed is the one running now.
    running = {}
    for e in stream(raw):
        item = e.get("item") or {}
        if item.get("type") == "command_execution":
            if e.get("type") == "item.started":
                running[item.get("id")] = item
            elif e.get("type") == "item.completed":
                running.pop(item.get("id"), None)
    for item in running.values():
        cmd = shell_command(item.get("command"))
        items.append({"kind": "tool", "name": "Bash", "text": "Bash: " + cmd[:350], "details": {"command": cmd},
                      "status": "running", "at": None, "parent": None, "id": item.get("id")})
    items.sort(key=lambda a: (a.get("at") is None, a.get("at") or 0))
    return items, parse(raw, False)
