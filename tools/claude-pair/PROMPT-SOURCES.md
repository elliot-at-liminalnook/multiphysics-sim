# Prompt sources and adaptations

Researched September 29, 2026. These are reference implementations, not installed
plugins or instructions to execute. The three role prompts contain original,
project-specific adaptations. The existing mission, user guidance, permissions,
independent checks and resource limits remain authoritative.

## Selection

Selected active public projects with published implementation prompts, substantial
community interest, and updates in the last few days. GitHub stars indicate
interest, not independently established reliability or suitability for Rust.

| Project | Stars at lookup | Latest main commit date | Source revision |
| --- | ---: | --- | --- |
| [obra/superpowers](https://github.com/obra/superpowers) | 292,953 | 2026-09-25 | `8ca22dba9a94f28898bbce59f2537ff4d87c747d` |
| [open-gsd/gsd-core](https://github.com/open-gsd/gsd-core) | 10,011 | 2026-09-29 | `ddde7191935f45074d1b08ff7a8dde8c5a21624a` |
| [bmad-code-org/BMAD-METHOD](https://github.com/bmad-code-org/BMAD-METHOD) | 53,644 | 2026-09-28 | `1cbcfa272fe65787c06a1fa164a901f46117cca7` |

The [old GSD repository](https://github.com/gsd-build/get-shit-done) is archived
and directs current development to Open GSD. Research used the new home.

## Superpowers: implementation and review

Read the [implementer prompt](https://github.com/obra/superpowers/blob/8ca22dba9a94f28898bbce59f2537ff4d87c747d/skills/subagent-driven-development/implementer-prompt.md)
and [task reviewer prompt](https://github.com/obra/superpowers/blob/8ca22dba9a94f28898bbce59f2537ff4d87c747d/skills/subagent-driven-development/task-reviewer-prompt.md).
Adapted a self-contained assignment, worker self-review, separate requirements
and quality judgments, focused review of named risks, and actionable repair
findings. Applied these inside the existing orchestrator review, without extra
reviewer agents. Retained independent coordinator checks. No blanket requirement
to run the entire suite before every commit or impose TDD on prompt/text edits.

## Open GSD: plan backward from behavior and verify the wiring

Read the [planner](https://github.com/open-gsd/gsd-core/blob/ddde7191935f45074d1b08ff7a8dde8c5a21624a/agents/gsd-planner.md),
[verifier](https://github.com/open-gsd/gsd-core/blob/ddde7191935f45074d1b08ff7a8dde8c5a21624a/agents/gsd-verifier.md), and
[wiring reference](https://github.com/open-gsd/gsd-core/blob/ddde7191935f45074d1b08ff7a8dde8c5a21624a/gsd-core/references/verifier-wiring-patterns.md).
Adapted planning from observable outcomes to implementation and evidence, and
checking that a new capability reaches its actual native-viewer consumer.
Kept binding decisions and unresolved outcomes visible across task splits.
Did not adopt parallel execution waves, rigid context-percentage rules or a
prohibition on modest initial slices.

## BMAD: carry context, challenge verification, learn between batches

Read [planning](https://github.com/bmad-code-org/BMAD-METHOD/blob/1cbcfa272fe65787c06a1fa164a901f46117cca7/skills/bmad-build/step-02-plan.md),
[verification-gap review](https://github.com/bmad-code-org/BMAD-METHOD/blob/1cbcfa272fe65787c06a1fa164a901f46117cca7/skills/bmad-build/review-prompts/verification-gap.md),
and [retrospective evidence gathering](https://github.com/bmad-code-org/BMAD-METHOD/blob/1cbcfa272fe65787c06a1fa164a901f46117cca7/skills/bmad-retrospective/references/evidence-gathering.md).
Adapted a concise code map in the worker assignment, regression checks at real
consumers, and an evidence-based Director retrospective before ranking the next
hopper. Missing evidence stays explicit. Kept process proportional to the task;
did not add BMAD's human approval checkpoints, install dependencies or generate
a competing collection of plan/status files.

## Anthropic: durable progress across sessions

Read [Effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents)
(November 26, 2025) and its [coding prompt](https://github.com/anthropics/claude-quickstarts/blob/main/autonomous-coding/prompts/coding_prompt.md).
This is older vendor guidance, included alongside the current community projects.
Adapted startup orientation using progress and Git history, one assignment at
a time, honest verification, and a durable end-of-turn handoff. Used our shared
notebook and existing report fields rather than a second feature/progress log.
Native Rust interaction remains the proof for native GUI claims; browser-only
tests, blanket staging and unrestricted history changes were not carried over.

## Installation and limits

Updated `prompts/director.md`, `prompts/orchestrator.md`, `prompts/worker.md`, and
the matching prompts in the saved viewer-pair state. Recorded the change in the
shared journal and operator guidance for the next orchestration turn. Refreshed
the portable bundle. The paused run was not resumed and no Claude calls were
made to evaluate these changes. Prompt installation is verified; improvements
to agent behavior still need observation during subsequent real tasks.
