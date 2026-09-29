# Annotation agents

`sim-agent` manages serial, durable Codex conversations without a renderer or
physics dependency. A host supplies `Input` (discussion ID, idempotency key,
source revision, question and JSON context), polls `Supervisor::snapshot`, and
applies a ready `Reply` through its own validated commands. Acknowledge with
`delivered` only after the deterministic run ID is present in the host document.

The worker starts `codex app-server --stdio` lazily, checks `model/list` for
the configured model and reasoning effort (`SIM_CODEX_MODEL`, default
`gpt-6-astra`; `SIM_CODEX_EFFORT`, default `high`; see `Config::from_env`), and
persists one session ID per discussion. It resumes that session on follow-up.
There is no silent model fallback: an unavailable model fails the run with an
error naming it. Answer mode uses a
read-only sandbox, never approves interactive tool requests, and instructs the
agent to inspect files and return a structured answer rather than make edits.
The application posts the reply; the model does not write the assembly file.

```sh
cargo run -p sim-agent --example answer -- \
  /absolute/project /tmp/annotation-demo/state.json \
  'Read AGENTS.md and explain the physical-model ownership rule.'
```

The example requires a logged-in local Codex CLI and model access. Set
`SIM_CODEX_EXECUTABLE` to an absolute CLI path when it is not on PATH.
The supervisor runs locally; Codex still sends model requests to its configured
service using the existing Codex login. This is not an offline model.

Persistence uses an atomic replacement with file and directory synchronization.
An exclusive Unix advisory lock prevents multiple supervisors claiming the same
state file. Restart preserves queued requests and completed replies. A turn
interrupted by a crash is marked failed for explicit retry, avoiding an
unrequested duplicate model invocation. A persisted reply awaiting delivery is
redelivered with the same ID, making host attachment idempotent.

The queue allows 32 outstanding requests, retaining up to 128 runs and 256
activity events. Human-comment keys remain recorded when old runs are evicted.
Protocol messages, annotation context and replies are bounded. The worker owns
all subprocess and persistence I/O; host snapshots exclude large context except
when a reply is awaiting attachment. Shutdown and cancellation interrupt/stop
the owned process group. A turn has a 30-minute limit and protocol calls have a
45-second limit. Authentication/model/protocol failures appear as retryable run
errors. Corrupt state is preserved and reported rather than overwritten.

The current app-server interface is experimental. The adapter intentionally
uses a narrow protocol surface and tests against a deterministic child process;
real-account smoke tests must also be run when upgrading Codex.

`cargo test -p sim-agent` covers queue deduplication/capacity, restart behavior,
real subprocess protocol exchange, conversation resume, automatic-answer
baseline, read-only/model settings, cancellation, and exclusive ownership.
