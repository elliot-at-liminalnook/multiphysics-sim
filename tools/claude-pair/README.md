# Claude pair: one Rust viewer

Two Claude Code roles cooperate through a small local coordinator, with an
optional third Director role choosing subsequent batches.
The orchestrator reads the project, maintains a migration checklist and writes
one bounded assignment. The worker implements it. The coordinator runs selected
checks independently, and the orchestrator reviews the files and results before
issuing the next assignment. The roles take turns; the worker implements.

This uses your installed `claude` command and existing Claude login. Model
inference is remote; the coordinator, source files and build commands run on
this Mac. It does not provision cloud machines or start Codex tasks.

## Start

Requires Python 3.9+, Git, and Claude Code with `--system-prompt-snapshot off` support (tested with 2.1.285).

```sh
python3 tools/claude-pair/pair.py init
python3 tools/claude-pair/pair.py run
```

Run these anywhere inside the project. The agents work **directly in this
folder** and commit to its current branch; there is no separate copy. Run state
(config, logs, journal, captures) lives in `.claude-pair/`, which `init` adds to
`.git/info/exclude`, so it never shows up in `git status` or commits. Every
other command finds it automatically. `--state` and `--repo` are only needed for
a different location.

`init` records a baseline commit for the folder as it is at that moment,
including uncommitted edits and non-ignored untracked files. It uses a temporary
index, so your HEAD, index and files are untouched. Reviews diff against that
baseline, so your pre-run work is never mistaken for agent work. The baseline is
pinned at `refs/claude-pair/<timestamp>/baseline`. `init` refuses if a run
already exists; `init --fresh` starts a new one and keeps the old run beside it
as `.claude-pair-<timestamp>/` (also ignored).

You can keep working in the folder during a run. The agents are told never to
revert, stash or commit changes they didn't make, and to stage only their own
files. Your concurrent edits will still appear in review diffs, and a build you
start can contend with theirs for the Cargo lock.

**There are no run limits by default.** The pair works until your weekly
Claude usage limit is used up: 5-hour limits pause it and it resumes after each
reset (see "Claude usage limits" below). No per-call time, turn or cost caps
apply, and the Director keeps choosing batches until it decides nothing is worth
doing. The only other stop is a safety pause when disk free space falls below
2 GiB. Calls also have no time limit, so a genuinely hung call waits until you
press Stop.

To add caps anyway, pass any of `init --max-rounds N --max-hours H --budget-usd D
--call-budget-usd D --turn-minutes M --max-turns N --no-director`, or fill in the
dashboard's Settings (empty means no limit). Omit `--model` to use
Claude's default; pass an explicit alias/ID if desired. No model is hardcoded.
The dollar figures are API-price usage estimates, **not a charge**. The
coordinator reconciles each session's reported total after every call; when a
dollar cap is set it reserves the call's cap first. The dashboard's 5-hour and
weekly meters come from Claude Code's own usage readings.

## Observe, stop and resume

Open the dashboard with:

```sh
python3 tools/claude-pair/open_dashboard.py --open
```

The header always shows what is happening now in one sentence, where the run is
in the five-stage loop (choose batch → assign → implement → check → review), and
meters for the checklist, worker turns, active time, usage estimate, Claude's
5-hour limit and the commits made so far. Below it:

- **Overview:** a live feed that follows whichever agent is working (tool calls,
  messages, prompt and structured result, with turn time and cost). Beside it
  are the current assignment, the acceptance checklist, check results (tagged
  *pre-existing* or *new failure*), screenshots from `ui_capture.py`, the run's
  commits, and a box for guiding the next step.
- **Timeline:** every agent turn, newest first, with duration, cost and status.
  Click one for its prompt, result, activity and errors.
- **Notebook:** the shared team journal, newest first.
- **Director:** the current batch, the hopper of compared candidates, and the
  automatic-planning switch.
- **Settings:** run limits, run details and the exact instructions each role gets.

It follows the system light/dark setting; the ◐ button overrides it. Guidance
reaches the next orchestrator turn, and a queued worker assignment is reconsidered
first. It does not interrupt a turn in progress.

The server binds only to loopback and requires a same-origin control token for
writes. It starts no agents until Continue/Start is clicked. Closing the browser
does not stop either the server or a running pair; use Stop for the agents.

```sh
python3 tools/claude-pair/pair.py status
python3 tools/claude-pair/pair.py stop
python3 tools/claude-pair/pair.py resume
```

`STATUS.md` gives the current phase and checklist. `state.json` records both
Claude session IDs, the latest assignment/report, check receipts and budget.
`logs/` preserves every prompt, raw response, worker report, review and test log.
New turns stream tool activity and public messages into their logs. Older
non-streaming turns show their prompt while running and their result afterward.
Private reasoning is not displayed. `--steps 1` pauses after one saved transition;
planning, working, verification and review are separate transitions.

Stop creates a stop marker; the coordinator terminates its active process group
and saves partial progress. Ctrl-C and SIGTERM also stop the child. If a Claude
call was interrupted or its outcome is uncertain, inspect its logs and the project folder
before `resume --retry-interrupted`. It resumes that session, preserving files;
it does not roll edits back. The full reservation remains in the usage ledger
when the actual cost is unknown, and recovery may overcount that cost. Never
delete the state to reset a budget. Run limits remain cumulative on resume.
An abrupt OS kill can leave a child alive: check for that process before retrying.

Only one coordinator may run per state directory. Keep the terminal/coordinator
running and the Mac awake for continued work; no daemon or scheduled automation
is installed. You can inspect a stopped session interactively with
`claude --resume SESSION_ID` in the project folder, but do not run it concurrently
with the coordinator.

## Scope and review

The mission is in `prompts/mission.md`. It makes the native viewer the primary
interface, preserves CAD source ownership and shared Rust execution, and requires
evidence before removing a legacy interface. Python/OCCT can remain a backend
dependency; the user should not need its separate UI for a migrated workflow.

### Full control

Every role runs with `--dangerously-skip-permissions` and the complete built-in
tool set: any shell command, file edits, subagents, web access, background
processes, builds and the native viewer. There is no command allowlist and no
permission prompt. Project settings, `AGENTS.md` and project skills load
(`--setting-sources project`). Your personal settings, hooks and MCP servers do
not, so a run behaves the same regardless of your interactive setup. Chrome
integration is off. A refused tool call (for example an interactive-only tool)
is recorded in the journal instead of stopping the run.

The only limits are a few hard boundaries stated in `prompts/mission.md`: no
hardware motion, no paid cloud, pushes, publication or purchases, no edits to
changes the agents didn't make (reverting, stashing or committing your work), no
deletion of experiments or other protected data, and no
tampering with coordinator state or receipts. These are instructions to the
model, not an OS sandbox: commands run as your user with your access.

### Fresh sessions

A role keeps its Claude session only while its scope is unchanged. The worker
resumes for `review=revise` repairs and starts fresh for each new assignment.
The orchestrator starts fresh for each batch. The Director is always fresh. Any
session is also replaced after `max_session_calls` turns (default 8, in
`config.json`). Fresh sessions work from the self-contained task contract, the
orchestrator's previous plan (included in its prompt) and the shared notebook.
The usage ledger reconciles each session's cumulative total separately.

### Verification: by reading, plus scheduled passes

Builds and tests here take minutes, so normal work doesn't run them. Every role
writes and reviews code by reading. Any command that builds or runs code must
finish within 10 seconds; the agents wrap such commands in `within 10 <command>`
(`bin/within`, on their PATH), which stops the command and its children and
reports a timeout as unverified, not failed. The worker fixes bugs it notices
while reading and commits them with how they were found.

**There are no build or test passes:** verification is by reading only, and
every epic stays focused on writing code. The verification-pass machinery is
still in the coordinator, off by default. Set `verify_every_commits` above 0 in
`.claude-pair/config.json` to schedule a build, bug-hunt and test pass every N
commits and before an epic completes, using `prompts/verification.md`.

The orchestrator may still list `checks` for the coordinator to rerun (normally
none). Each is stopped at `check_seconds` (default 10); a timeout counts as
unverified, not failed. They run cheapest first and stop at the first new
failure. Acceptance needs them to pass or time out, except those in
`waived_checks` (allowed only for a check already failing in an earlier
assignment).

Review evidence includes a diffstat, commits with stats, untracked files and
recent captures alongside the full diff file, so the orchestrator can read the
relevant parts. The diff compares the baseline with everything in the folder
now, so new untracked files are included and your pre-run untracked files never
appear as deletions.

### Native UI capture

`ui_capture.py` launches the native viewer (`sim-spatial`) on a free loopback
port and runs a JSON script of REST commands (`system_ui` activates live
controls through the same handlers as a click). It saves window screenshots
exactly as drawn, writes `capture.json` and stops the viewer. It exits nonzero
if any step fails, so the worker can use it as proof (or the orchestrator as a rerun):

```sh
python3 tools/claude-pair/ui_capture.py --out "$PAIR_CAPTURES/board" \
  --steps '[{"command":"display","args":{"action":{"kind":"set_exploded","enabled":true}}},{"screenshot":"exploded"}]' \
  -- --system examples/systems-builder/motor-driver-board/board.system.json
```

Agents view the PNGs with Read. The viewer opens a real window, so a run needs
a logged-in desktop session. A REST-activated control is evidence of the UI
handler and what it draws, not of a mouse gesture. Build the viewer in the
viewer first; an older binary may lack newer commands.

### Completion

Completion requires an accepted worker result and evidence for every checklist
item. The orchestrator must inspect untracked files separately from the Git diff.
No pushes or deployments are performed. The worker commits each small
completed task to the folder's current branch, and reviews and whitespace checks
cover those commits against the run's baseline. Commits are not gated on
acceptance. If you want to review before anything lands on `main`, check out a
branch before `init`.

`init --audit-only` gives every session read-only
tools (Read, Glob, Grep, no Bash) and only permits the diff check, useful for
inventory without code edits.

## Continuous improvement: the Director and hopper

Enable the outer loop in the dashboard, or use:

```sh
python3 tools/claude-pair/pair.py enable-outer   # optional --max-batches N
```

New runs start with it on and no batch ceiling (`init --no-director` turns it off).

The Director is a third Claude role, always started in a fresh session. It compares 3–6
source-backed candidates across at least two of four categories: cohesion,
feature gaps, shared-library improvements, and technical debt. It explains the
benefit, effort, risk, and reason for choosing or deferring each candidate. It
selects one batch of 1–4 tasks with observable outcomes and named checks.

Taste is expressed as concrete rules in `prompts/director.md`: finish real user
workflows, reduce competing concepts and implementations, require real consumers
for reusable code, and address named recurring costs instead of cosmetic churn.
This guides model judgment; it does not guarantee good product decisions.

The inner pair executes only the selected batch. Completion requires an accepted
worker report, passing requested reruns (if any), and evidence for every batch task and
outcome. Only then is the batch archived and the Director called again. It
reconsiders the hopper against the current code instead of blindly draining an
old queue. Failed checks, blocked work and interruptions do not refill it. The
Director may stop with a reason when no candidate is worthwhile.

All three roles share any time, usage and worker-turn limits you set. An
optional completed-batch ceiling is editable in the dashboard (empty: none). It includes an adopted in-progress assignment but excludes the
archived legacy whole-mission completion. Disabling automatic planning lets the
current batch finish and pauses before selecting another one. Stop interrupts the
active call as before. Neither control raises a budget.

Existing runs upgrade under the exclusive coordinator lock. An already-running
old coordinator cannot load new Python code. A small background upgrade watcher
waits for it to exit without interrupting its worker. On successful mission
completion it starts the Director automatically; on pause, failure, user stop or
limits it prepares the saved state but does not resume work. The next Continue
uses the new code. The dashboard labels this interval **Upgrade queued**. The
watcher is not a scheduled automation and does not survive a machine restart;
Continue still performs the migration after a restart.

When adopting unfinished work, the existing assignment and evidence are kept.
Its acceptance criteria become the first bounded batch, while the old full-project
checklist is preserved as a historical roadmap. Agents must recheck its freshness.
The Director's current prompt, activity, reasoning summary, hopper, selected tasks,
and completed batches with review receipts are visible in the dashboard. Older
batch decisions also remain in the conversation history and log files.

## Codex backend

Every role can run on Claude Code (the default) or OpenAI's Codex CLI, and you
can switch at any time, per role or for all of them:

```sh
python3 tools/claude-pair/pair.py backend codex                  # every role
python3 tools/claude-pair/pair.py backend claude --role director # one role
python3 tools/claude-pair/pair.py backend --clear-role director  # follow the default again
python3 tools/claude-pair/pair.py backend --codex-model gpt-6-sol --codex-effort high
```

or use the **Claude | Codex toggle in the dashboard header** (all roles; a
dashed outline means roles differ) or **Settings → Agent backend** (per role,
plus the Codex model and reasoning). The choice is read at each call, so it
works while a run is going: a turn in progress finishes on the backend it
started on, and the role's next call uses the new one. Settings shows, per
role, where it runs now and where its next call goes.

**Code changes and restarts.** A running coordinator keeps the code it started
with. It records a fingerprint of that code, and when the files in
`tools/claude-pair` change the dashboard says so and offers **Restart after
this turn**: the coordinator stops between turns, re-executes itself on the
new code and carries on, interrupting nothing (`pair.py restart` does the same).
A coordinator started before this existed offers **Restart now** instead: Stop,
then Continue at once, with the turn in progress resuming in its own session.
Prompt files never need a restart.

**Automatic switching.** Before each call the coordinator checks both
backends' latest usage readings (Claude's 5-hour and weekly windows from its
stream, Codex's from its session rollouts). When the backend a role is set to
has used 95% of a live window and the other backend has room and is installed,
that call runs on the other backend; when the window resets, the role goes back
by itself. Your setting is never rewritten, and each switch and return is
journalled once. A usage limit actually hit does the same at once, instead of
waiting for the reset. If both backends are nearly used up, or a backend
reports a limit again right after a switch, the run waits or pauses exactly as
before, so it never switches back and forth. Turn it off or move the point with
`pair.py backend --auto-switch off` / `--switch-at 90`, or in Settings. The
dashboard shows a usage meter per backend and a banner while a switch is in effect.

**Continuity.** Everything that carries the run (batch, plan, checklist,
assignment, reports, journal, decisions, ledger) belongs to the coordinator, not
to either CLI. Sessions are the one thing that can't move: each session is
tagged with the backend that started it and is only ever resumed there. A role
whose backend changed starts a fresh session, exactly as it does for a new
assignment, from the task contract and the notebook. A turn that was cut short
(Stop, a crash, a usage limit, a failed call) and then switched is told it is
continuing on the other backend and to check the folder for its partial work.
`test_backends.py` locks this in for every direction and situation.

**How each feature maps** (verified against codex-cli 0.157.1 on 2026-10-01):

| | Claude Code | Codex CLI |
|---|---|---|
| Call | `claude --print --output-format stream-json` | `codex exec --json`, prompt on stdin |
| Structured result | `--json-schema` | `--output-schema` (the same three schemas) |
| Role instructions | `--append-system-prompt`, re-sent every call | `developer_instructions` on a new session; on resume Codex keeps the old ones, so changed instructions are restated at the top of the prompt |
| Personal setup kept out | `--setting-sources project`, empty MCP, `--no-chrome` | `--ignore-user-config`; memories, browser, computer use and apps off |
| Permissions | `--dangerously-skip-permissions` | `--dangerously-bypass-approvals-and-sandbox` |
| Audit-only | read-only tools | read-only sandbox, no subagents |
| Subagents | `--agents prompts/subagents.json` | the same file, as `pair_implementer` / `pair_reviewer` roles (`-c agents.<name>.…`, files in `.claude-pair/codex-agents/`) spawned with `spawn_agent` |
| 10-second shell rule | Claude Code stops each command | no such setting: `shims/cargo` runs cargo under `within`, and the role is told to wrap other slow commands |
| Fast mode | `fastMode` setting for `fast_roles` | off unless `codex.fast` is on (`backend --codex-fast on`, or Settings); then `service_tier = "priority"` for `fast_roles`. Off by default because the priority tier uses the allowance faster |
| Usage limits | `rate_limit_event` in the stream | `rate_limits` in the session rollout (`~/.codex/sessions`), shown on the same meters |
| Cost | dollar estimate per session | tokens only, so the dollar caps don't stop Codex calls (the journal says so); the turn time limit applies to both |
| Model-turn cap | `--max-turns` | none (the journal says so) |
| Dashboard | stream events | stream plus rollouts; subagent lanes come from their own rollouts. Codex encrypts subagent briefs, so a Codex lane shows its role, actions and reply but not its brief |

**Setup.** `codex login` once, and keep the CLI current (`codex update`):
0.157.1 lacked `gpt-6.1-sol`; 0.160.0 has it and was verified end to end on
2026-10-01 (fresh call, exact instructions, resume after an instruction change,
a `pair_reviewer` subagent, fast mode). `--codex-executable` points the run at
another binary. Without `--codex-model` Codex uses its default.

## Verify the coordinator

```sh
cd tools/claude-pair
python3 -m unittest -v test_pair.py test_dashboard.py test_outer.py test_notebook.py test_workflow.py test_control.py test_backends.py
```

Tests exercise optional coordinator reruns, the in-place baseline (staged/unstaged/deleted/untracked files
and links, with HEAD, index and files untouched), state exclusion, `--fresh`, prompt forwarding, resume, final review, run limits, rejected
false completion, check gating, locking, stop handling and usage accounting.
`test_control.py` covers the full-control flags, audit-only read-only mode,
session scoping and caps, shell checks and their environment, before/after
receipts and waivers, evidence summaries, the pinned baseline, and `ui_capture.py`
against a fake viewer.

Implementation references: [Claude programmatic mode](https://code.claude.com/docs/en/headless)
and [CLI reference](https://code.claude.com/docs/en/cli-reference). Cost reports
are cumulative per session, while `--max-budget-usd` applies to the current call.

## Fast mode

The worker runs in Claude Code's fast mode by default (`fast_roles` in
`.claude-pair/config.json`, or `init --fast-roles worker orchestrator ...`). The
coordinator passes `--settings '{"fastMode": true}'` for those roles and reads
the setting at every call, so edits apply at the next turn. Each turn's footer
shows whether it ran fast. If Claude Code refuses fast mode (an organization
setting or usage state), the turn runs at normal speed and the journal notes why.
Fast mode is priced higher, so it uses the 5-hour and weekly allowances faster.

## Subagents

Every session gets two custom subagent types from `prompts/subagents.json`
(passed with `--agents`, so they don't appear in your own Claude Code
sessions):
- `pair-implementer` builds one self-contained part of an epic.
- `pair-reviewer` hunts bugs in a diff by reading.

Both carry the run's rules: read instead of build, 10-second commands, no
screenshots, the architecture document, and no commits. The worker is told to
split epics across them by making several `Agent` calls in one message, which
run in parallel. That was verified under the run's settings: background tasks
are off, so subagents run in the foreground, but calls made together still
overlap. It then reviews and commits the combined result. Subagents inherit the
10-second shell cap. Claude Code allows 20 running at once and nesting 3 levels
deep, and their usage counts toward the same Claude limits.

## Decisions instead of stops

The run is meant to keep going unattended. Agents decide rather than wait: an
ambiguous requirement, a stale fixture or two reasonable designs are settled by
the agent. Each such choice is recorded in its response's `decisions` field
(decision, why, alternatives, revisit-if). The coordinator collects them in the
dashboard's **Decisions** tab and `.claude-pair/shared/DECISIONS.md`, and adds
them to the journal. Revisit any of them with the guidance box.

The coordinator also avoids stopping on problems it can handle:
- **A blocked batch** is set aside with its blockers, and the Director chooses
  other work (it reselects a set-aside batch only once the blocker is resolved).
  Without a Director, a blocked orchestrator still ends the run.
- **A failed call** (an API error, a crash, a result that doesn't match the
  schema) is retried in the same session after a backoff of 30 s, doubling up to
  30 min, and the run continues. It stops after 8 consecutive failures
  (`failure_retries`, `failure_backoff_seconds`).
- **A response that breaks a handoff rule** is sent back to the same session to
  correct (`guard_retries`, default 2).
- **An interrupted turn** (Stop, a crash, a closed terminal) resumes its own
  session automatically on the next start.

What still stops it: your Stop, the weekly usage limit, the Director deciding
nothing worthwhile remains, less than 2 GiB of free disk, and a coordinator bug.

## Claude usage limits

When a call is stopped by a Claude usage limit, the run does not fail. **The
weekly limit ends the run:** it pauses with the reset time shown, and Continue
after the reset picks up the interrupted session (set `wait_for_weekly_limit:
true` in `config.json` to wait through it automatically instead). For the 5-hour
window or a spend limit, the coordinator reads the reset
time from Claude Code's `rate_limit_event` stream data, falling back to the error
text and then to a 15-minute retry (`limit_retry_minutes`). It marks the run
**waiting**, sleeps until the reset plus a minute (`limit_margin_seconds`), then
resumes the interrupted agent's session with a note to continue where it
stopped. A call that hadn't started any work is simply rerun. Waiting time does
not count against the run's active hours, the cut-short attempt is not counted
as a worker turn, and its usage is reconciled from what Claude reported.

Stop still works while waiting. If the coordinator process itself exits during
the wait (terminal closed, crash), the resume time is saved: the next `run`
waits out the remainder first, and an open dashboard relaunches the run once the
reset has passed. Nothing restarts after a reboot unless the dashboard or `run`
is started again, and the Mac must be awake for the run to continue.

## Shared team notebook

All three roles share the project folder and a coordinator-maintained
`shared/` directory in the run state. `SYSTEM.md` explains the roles, serial
handoff loop, authority and evidence rules. `CURRENT.md` holds the current batch,
assignment, report, check receipts, limits and user guidance. `journal.jsonl` is
the canonical append-only chronological log; `JOURNAL.md` is a readable projection.

Every new agent call receives the common system guide, absolute paths to shared
context, and bounded recent journal excerpts. The full log stays readable by every
role; the worker receives access to the shared directory too. Role schemas include
`coordination_notes: string[]`. Notes can address another role, raise questions,
record decisions or explain artifacts and cleanup. Use entry IDs when replying.
The coordinator appends the agent's summary and notes after its successful response
returns, before dispatching the next agent. Notes from
an interrupted turn are not claimed as delivered; public live activity remains in
its transcript and the workflow panel.

Entries have authors, sequence numbers, timestamps, source transcripts and stable
IDs. Duplicate IDs do not create duplicate entries. A locked single writer and
flushed writes preserve ordering; malformed journal data stops the write for
inspection instead of silently discarding history. The Markdown projection is
rebuildable. Agents must not edit notebook files directly. This is a workflow
contract, not a separate operating-system security sandbox.

Agent reports/proposals are distinct from coordinator verification and validated
handoff entries. Notes cannot change permissions, expand a batch, raise limits,
or establish success without evidence. Unanswered questions remain unresolved.
Completed pre-notebook responses are imported and labelled as historical.

The dashboard's Team notebook shows the latest 80 entries in chronological order,
with the full journal, common guide and current context available on demand.
New notes appear on the regular refresh. This does not start or resume agents.

## Task pacing and local commits

The orchestrator favors small complete assignments with clear stopping points;
the Director can refill the hopper later. Workers make small local commits after
focused task checks pass, stage only their task changes, and report commit IDs and
remaining edits. They do not push or rewrite history. These checkpoints remain
subject to orchestrator review against the run's baseline.

## Prompt research

See [PROMPT-SOURCES.md](PROMPT-SOURCES.md) for published workflow prompts,
source revisions and the specific adaptations to our three roles. These are
prompt refinements; behavioral benefits require observation on real tasks.
