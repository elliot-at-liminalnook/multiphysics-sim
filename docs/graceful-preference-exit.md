# Ordinary native closure — T47

This batch is implemented and verified by source reading only. Compilation,
fixtures, interactive usability and platform behavior are unexecuted. The launch
path remains `cargo run -p sim-spatial -- <document>`; no new viewer is introduced.
Use the global Close viewer control or the ordinary window close button. During
closure the same global panel reports preservation blockers and preference work.
Cancel close retains drafts and the settings jobs; Retry preferences uses the
existing retry owner. Exit without saving preferences acknowledges only the
current preference scope. It cannot discard study evidence or unresolved study
text. A later preference edit, recent record or startup landing invalidates that
acknowledgment. No CAD save or study recovery file is written automatically.

## Owners and source map

| Entry or resource | Source and contract |
|---|---|
| Window close | Pinned `bevy_winit-0.19.1/src/state.rs` maps `WindowEvent::CloseRequested` to `WindowCloseRequested`; `app/close.rs::window_input` writes the same typed action as the rendered button. |
| Ordinary decision | `app/close.rs::CloseOwner` retains intent, scoped acknowledgment and two-frame authorization; its apply system is the only action owner. The former `guarded_close` is removed. |
| Rendered controls | `app/close/ui.rs` spawns persistent kit Button/CloseAction/Enabled entities across every mode; `controls_in` reads those actual entities. |
| REST and system_ui | `app/actions.rs::registry` registers close commands; `app/route.rs::route` routes `close_*` and `close:*` activation to that action owner. Common annotation adds actual rendered controls. |
| Preference publication | `app/settings/mod.rs::SettingsOwner` retains revisions, record queue, error state and jobs; `settings/plugin.rs` lands results in public `SettingsSet::Publish` within JobResults. |
| Study preservation | `StudyUi::blocking_reason` retains active, queued, awaiting and parked field drafts. `StudyOwner::blocking_reason` retains dirty or pending evidence. Closure rechecks both after Update. |
| Immediate safety | `robot/hardware/actions.rs::request_close_stop` uses existing immediate STOP and owned-session stop paths before any preference wait. Hardware Actions precedes public CloseSet::Apply; pending closure refuses new motion actions. |
| Independent safety | `hardware/actions/input.rs::window_loss`, `actions.rs::stop_on_exit`, and `link.rs` bounded shutdown/Drop remain independent. Existing synchronous shutdown fallback bounds are unchanged. |
| CAD release | `cad/sync/mod.rs::on_exit`, after ExitSystems, calls `CadDocument::release_child`. It detaches a potentially unsaved self-started service, logs its URL, and closes its child slot. A late startup cannot put a child into the closed slot. Attached services are never stopped. |
| Parked documents | `app/switch/leave.rs` parks Inspect and Build scenes. CAD is released, not parked; replacement in `switch/arrival.rs` also uses release_child. No second CAD child owner is introduced. |

Public Update order remains Input → Actions → JobResults → SimSync → Present.
CloseSet::Apply and CloseSet::Publish expose close action/projection ordering.
The final Last authorization before Bevy ExitSystems reads current settings and
preservation facts again, rather than trusting a rendered snapshot. It marks
ClosingWindow only when ready, then rechecks the same preference stamp before
despawning on the following frame. New work disarms closure. Bevy ExitSystems
writes AppExit only after the window entities have gone. Feature AppExit readers
release resources; they do not own another close decision.

## Drain reading traces

Close during loading keeps Window alive. Startup landing invalidates an earlier
preference-loss acknowledgment. Usable session-only projections, protected
schemas/sources and unavailable config destinations never imply durability.
Accepted recent documents stay required until normalized and included in an
acknowledged publication. Normalization errors retain the queue front and wait
for retry; absent paths retain their absolute spelling, preserving existing
recent-document compatibility. Other normalization errors fail visibly.

An active save owns an immutable captured revision. Its completion must match
that captured identity; it acknowledges only that revision. A newer edit remains
dirty. Superseded publication returns an error rather than returning an unrelated
higher revision. Failed publication and snapshot errors cannot become Ready.
Retry changes persistence intent without modifying authored documents. Cancel
close cancels only lifecycle intent, leaving queued records, jobs and drafts owned
by their existing features. Polling does not publish semantic changes on idle
frames. Disk operations remain jobs-owned and frame code does not wait for jobs.

## Interception limits

Local pinned sources were available and read: Bevy window `lib.rs`/`system.rs`,
winit runner `state.rs`, ECS message/change detection and UI signatures, plus
winit 0.30.13 macOS termination handling. Ordinary CloseRequested is delayable
because CorePlugin disables Bevy's stock close_when_requested system. Arbitrary
AppExit is not delayable by observing it: the runner's `app.should_exit()` calls
event_loop.exit. Direct Window destruction likewise bypasses this owner.

macOS Cmd+Q uses Cocoa `terminate:`; winit's `will_terminate` closes native windows
and enters LoopExiting. Bevy `exiting` clears native windows and `world.clear_all()`.
This is not an acknowledged ordinary close drain. Resource Drop safety remains
best effort; CAD detachment through the AppExit reader is not guaranteed on this
path. Crashes, SIGKILL, abort and power loss cannot preserve in-memory drafts or
guarantee preference publication. SettingsOwner::drop remains a best-effort
jobs submission, never evidence that an ordinary close succeeded.

## Batch checklist and evidence limits

| ID | Reading evidence |
|---|---|
| graceful-preference-exit:outcome-1 | Typed close action, durable owner, common registry/route and actual control projection. |
| graceful-preference-exit:outcome-2 | Immediate adapter, public action ordering, retained loss/AppExit/Drop safety. |
| graceful-preference-exit:outcome-3 | Settings drain states, required record retention and exact captured save acknowledgment. |
| graceful-preference-exit:outcome-4 | Global kit retry/cancel/scoped exit controls; shared owner revalidates preservation. |
| graceful-preference-exit:outcome-5 | Former guard replaced; sole-owner/source/platform contracts recorded here. |
| graceful-preference-exit:task-T47.1 | Close owner, feature preservation facts, Last recheck and unchanged CAD release contract. |
| graceful-preference-exit:task-T47.2 | Nonblocking settings drain and real kit control components. |
| graceful-preference-exit:task-T47.3 | Written isolated settings/window/control fixtures and independent source review. |

T47.1–T47.3 evidence consists of source traces and written fixtures, not executed
receipts. Existing CAD lifecycle fixtures inspect release_child and on_exit with
dirty/unconfirmed child ownership and startup-slot refusal. No hardware operation,
build, test, launch, export, screenshot or parity execution occurred.

Precise final trace anchors (paths relative to `crates/sim-spatial/src`):
`app/close.rs:75` maps window requests; `:100` applies typed intent and STOP;
`:132` rechecks and authorizes destruction. `app/settings/mod.rs:261` projects
drain states; `app/settings/plugin.rs:124` retains failed records and `:221`
checks captured publication acknowledgment. `app/close/ui.rs:11` discovers real
controls and `:69` renders them. `app/actions.rs:352` registers close and `:508`
adds rendered controls; `app/route.rs:45` routes close requests in every mode.
`robot/hardware/actions.rs:481` requests immediate close STOP and `:532` refuses
new motion while pending. `cad/document/state.rs:276` remains the release owner.
Written fixtures live in `app/close/tests.rs`, `app/close/ui_tests.rs`,
`app/settings/drain_tests.rs`, `robot/hardware/actions/tests.rs:6` and
`cad/lifecycle_tests.rs:118`. Two independent reviewers read publication races
and lifecycle/UI/CAD/hardware paths. A missing Replies resource in two new
integration fixtures was found and repaired; the repair was independently read.

T46 was accepted by source review in call-0360 across 1b0ca533, d238d6b6 and
c380210b. Its three repair findings were accepted; compilation, written fixtures,
interactive usability and executed export/parity remain unverified. Historical
uncompiled-epic counts are snapshots, not evidence of later compilation. This
batch does not retire Python, browser or sim-viewer compatibility paths.


## T51 visible publication and close draining

The [shared publication contract](shared-evidence-publication.md) changes filesystem
mechanics beneath SettingsOwner, leaving CloseOwner as the sole ordinary close
authority. A visible replacement with unconfirmed synchronization remains dirty and
projects PublicationFailed. Read-back alone cannot authorize closure. Retry validates
the expected snapshot, preserves any observed external edit, and confirms required
synchronization before `land_save` may acknowledge the captured revision.

The publication gate tracks a visible revision floor separately from its confirmed
revision; an older unconfirmed replacement cannot displace newer visible work.
Late acknowledgments remain tied to their captured revision and cannot authorize a
newer settings state. Close authorization still rereads current `drain_stamp`, current
settings readiness and StudyOwner/StudyUi blockers at Last on both authorization
frames. Revision changes revoke armed closure and preference-loss acknowledgment.
Complete-on-drop and abrupt-exit limitations above remain unchanged. Shared injected
failure and drain-revision fixtures are source-written, uncompiled and unexecuted.
