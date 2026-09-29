# Claude pair: one Rust viewer

Two persistent Claude Code sessions cooperate through a small local coordinator,
with an optional third read-only Director session choosing subsequent batches.
The orchestrator reads the project, maintains a migration checklist and writes
one bounded assignment. The worker implements it. The coordinator runs selected
checks independently, and the orchestrator reviews the files and results before
issuing the next assignment. The sessions take turns; only the worker edits.

This uses your installed `claude` command and existing Claude login. Model
inference is remote; the coordinator, source files and build commands run on
this Mac. It does not provision cloud machines or start Codex tasks.

## Start

Requires Python 3.9+, Git, and Claude Code with `--system-prompt-snapshot off` support (tested with 2.1.284).

```sh
python3 tools/claude-pair/pair.py init \
  --repo /Users/elliot/physics-simulator \
  --state /Users/elliot/Documents/Codex/viewer-claude-pair

python3 tools/claude-pair/pair.py run \
  --state /Users/elliot/Documents/Codex/viewer-claude-pair
```

Run these from the physics-simulator root. `init` captures the current working
files, including staged, unstaged and non-ignored untracked work, in a separate
Git worktree. Your source HEAD, index and working files stay intact. The snapshot
has its own baseline commit so later diffs show only the worker's changes.
On APFS, copies use copy-on-write. Git must still hash/compress new source data;
large experiment collections can make initialization slow. Ignored build
outputs and caches are not copied. External symlinks and nested repositories
require explicit handling and cause initialization to stop.

The default run ends after **12 worker turns, 8 active hours, or $100 of estimated
model usage**, whichever applies first. Each Claude call also has a 45-minute,
100-turn and $10 estimated-usage limit. A final review is allowed after the last
worker turn, within the other limits. The process pauses before disk free space
falls below 2 GiB. These are bounded defaults, not an estimate of migration cost.

Change limits with `init --max-rounds N --max-hours H --budget-usd D
--call-budget-usd D --turn-minutes M --max-turns N`. After initialization,
edit the same fields in `config.json` only while stopped. Omit `--model` to use
Claude's default; pass an explicit alias/ID if desired. No model is hardcoded.
The CLI's dollar figures are API-price usage estimates, **not a charge or a
measure of subscription tokens remaining**. Claude enforces each call's limit;
the coordinator keeps an aggregate ledger, reserving the full call limit before
launch and reconciling cumulative session totals afterward. Estimates/caps can
overshoot by an in-flight model response; this is not a billing guarantee.

## Observe, stop and resume

For a live control panel, run:

```sh
python3 tools/claude-pair/open_dashboard.py \
  --state /Users/elliot/Documents/Codex/viewer-claude-pair --open
```

The local dashboard shows the mission, both exact prompts, live activity for new
turns, decisions, results, migration checklist, saved conversations and check
receipts. It includes stop/continue controls, adjustable limits, and operator
guidance. New guidance reaches the next orchestrator turn. If a worker task is
queued but hasn't started, the coordinator asks the orchestrator to reconsider
that assignment first. Guidance does not interrupt a turn already in progress.
The main mission and safety/source boundaries remain in force.

The server binds only to loopback and requires a same-origin control token for
writes. It starts no agents until Continue/Start is clicked. Closing the browser
does not stop either the server or a running pair; use Stop for the agents.

```sh
python3 tools/claude-pair/pair.py status --state /Users/elliot/Documents/Codex/viewer-claude-pair
python3 tools/claude-pair/pair.py stop --state /Users/elliot/Documents/Codex/viewer-claude-pair
python3 tools/claude-pair/pair.py resume --state /Users/elliot/Documents/Codex/viewer-claude-pair
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
call was interrupted or its outcome is uncertain, inspect its logs and workspace
before `resume --retry-interrupted`. It resumes that session, preserving files;
it does not roll edits back. The full reservation remains in the usage ledger
when the actual cost is unknown, and recovery may overcount that cost. Never
delete the state to reset a budget. Run limits remain cumulative on resume.
An abrupt OS kill can leave a child alive: check for that process before retrying.

Only one coordinator may run per state directory. Keep the terminal/coordinator
running and the Mac awake for continued work; no daemon or scheduled automation
is installed. You can inspect a stopped session interactively with
`claude --resume SESSION_ID` in its workspace, but do not run it concurrently
with the coordinator.

## Scope and review

The mission is in `prompts/mission.md`. It makes the native viewer the primary
interface, preserves CAD source ownership and shared Rust execution, and requires
evidence before removing a legacy interface. Python/OCCT can remain a backend
dependency; the user should not need its separate UI for a migrated workflow.

The orchestrator has only Read, Glob and Grep. The worker has file tools and
Bash in Claude's automatic permission-review mode. There is no permissions
bypass. Hooks, external MCP servers, Chrome integration and skills are disabled
for these sessions; role/mission prompts are supplied explicitly. Denied actions
stop the handoff and are recorded. This is a workflow boundary, not an OS security
sandbox: the worker's commands still run as your user. It is instructed not to
touch the source checkout, coordinator, hardware, remote services or home settings.

`checks.json` is a catalogue of exact argument arrays; the orchestrator selects
names, not arbitrary shell text. It is copied into the run configuration at
initialization. Add focused checks there while stopped as needed. Checks use
the isolated workspace and its build cache. The worker may run additional checks,
but those reports are distinguished from independent coordinator receipts.

Completion requires an accepted worker result and evidence for every checklist
item. Failed checks cannot be accepted by the state machine. The orchestrator
must inspect untracked files separately from the Git diff. Native UI parity is
still a model-reviewed claim and needs real interaction evidence; this tool does
not automate screenshots or provide a GUI testing rig. If that evidence isn't
available, the agents must report it as unverified. No automatic merges, source
checkout updates, pushes or deployments are performed. The worker is encouraged
to make small local commits of completed tasks in the isolated checkout; reviews
and whitespace checks include those commits against the original baseline.
Review the isolated changes before integrating them into your ongoing work.

`init --audit-only` still creates a snapshot but gives both sessions read-only
tools and only permits the diff check, useful for inventory without code edits.

## Continuous improvement: the Director and hopper

Enable the outer loop in the dashboard, or use:

```sh
python3 tools/claude-pair/pair.py enable-outer \
  --state /Users/elliot/Documents/Codex/viewer-claude-pair --max-batches 8
```

The Director is a third persistent, read-only Claude session. It compares 3–6
source-backed candidates across at least two of four categories: cohesion,
feature gaps, shared-library improvements, and technical debt. It explains the
benefit, effort, risk, and reason for choosing or deferring each candidate. It
selects one batch of 1–4 tasks with observable outcomes and named checks.

Taste is expressed as concrete rules in `prompts/director.md`: finish real user
workflows, reduce competing concepts and implementations, require real consumers
for reusable code, and address named recurring costs instead of cosmetic churn.
This guides model judgment; it does not guarantee good product decisions.

The inner pair executes only the selected batch. Completion requires an accepted
worker report, passing independent checks, and evidence for every batch task and
outcome. Only then is the batch archived and the Director called again. It
reconsiders the hopper against the current code instead of blindly draining an
old queue. Failed checks, blocked work and interruptions do not refill it. The
Director may stop with a reason when no candidate is worthwhile.

All three sessions share the existing cumulative time, usage and worker-turn
limits. The additional completed-batch ceiling defaults to eight and is editable
in the dashboard. It includes an adopted in-progress assignment but excludes the
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

## Verify the coordinator

```sh
cd tools/claude-pair
python3 -m unittest -v test_pair.py test_dashboard.py test_outer.py
```

Tests exercise snapshot isolation (including staged/unstaged/deleted/untracked
files and links), prompt forwarding, resume, final review, run limits, rejected
false completion, check gating, locking, stop handling and usage accounting.

Implementation references: [Claude programmatic mode](https://code.claude.com/docs/en/headless)
and [CLI reference](https://code.claude.com/docs/en/cli-reference). Cost reports
are cumulative per session, while `--max-budget-usd` applies to the current call.

## Live workflow map

The dashboard now opens with a five-step map: Director selection → assignment →
implementation → independent checks → review. It follows the saved coordinator
phase and distinguishes running, paused, blocked and historical activity.

Hover or keyboard-focus a step to preview it; click to pin its details. **Follow
current** returns to automatic tracking. The detail panel shows the exact prompt,
recent public messages/tool activity, call number, turn time, last log update,
reported model when available, acceptance progress and overall usage estimate.
The usage estimate excludes unresolved active/interrupted-call reservations;
full budget accounting remains in the run state and existing usage display.

Independent check output appears while verification is in progress and remains
separate from completed pass/fail receipts. Paused turn timing stops at the last
recorded output rather than continuing to count idle time. Live output refreshes
with the dashboard every two seconds; absence of output does not prove a hang.
Longer objective and Director sections are expandable below the map.

## Shared team notebook

All three roles share the isolated project checkout and a coordinator-maintained
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
returns, before dispatching the next agent. Planners remain read-only. Notes from
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
subject to independent checks and orchestrator review against the original snapshot.

## Prompt research

See [PROMPT-SOURCES.md](PROMPT-SOURCES.md) for published workflow prompts,
source revisions and the specific adaptations to our three roles. These are
prompt refinements; behavioral benefits require observation on real tasks.
