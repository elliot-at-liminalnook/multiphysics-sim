# Icons, display placement and discussions

**Builder placement is display-only.** Moving, snapping, grouping for layout,
or pinning a comment does not edit CAD geometry, joint frames, contact geometry,
mass, inertia, or simulation initial conditions. Use CAD and physical model
parameters for those changes. The REST responses expose `display_semantics`
or `semantics` explicitly; coordinates are metres, right-handed, +Y up, in the
enclosing subsystem definition's frame. This differs from CAD's millimetres.

## Place components

The library, outline and component inspector use shared vector icons. Their
names and paths are defined in `sim_core::icons`; registry entries expose the
chosen name as `icon`. Authored `.part` files may say `icon motor` (or another
name from `sim_core::icons::NAMES`). Saved subsystem definitions may set `icon`.
Unknown native types get a generic component icon and retain their text label.

Drag a library row into the viewport to place it. Drag an existing part to
move it across the current workplane. Multi-selection preserves relative
offsets. Drag an axis handle, or press X/Y/Z during a drag, to constrain motion.
Alt temporarily bypasses snapping; Escape cancels. Release in the viewport to
commit one shared undo step; dropping outside cancels. The preview changes
neither the saved document nor simulation parameters.

The Library tab has Grid visibility, Snap, XY/XZ/YZ plane, spacing and origin.
Spacing is displayed in millimetres; numeric editors accept metres. The
inspector provides exact x/y/z entry for the selection anchor. Grid settings
are saved on the definition, so repeated instances share the same local grid.
Moving children inside a shared definition affects each of its occurrences;
use Make unique first when an occurrence should differ.

## Overlap rejection

A placement is rejected if it creates or increases interference between
unrelated parts. Drag previews outline conflicts in red; releasing an invalid
preview restores the source placement without creating an undo entry. Touching
faces are allowed (1 micrometre tolerance). Existing authored overlaps may
remain or decrease, and moving a selection together preserves its internal
layout. A newly placed subsystem retains its authored internal layout while
its leaves are checked against the surrounding assembly.

A direct rotational or translational port connection permits overlapping
mates, including connected subsystem envelopes. Electrical and signal wiring
does not grant that permission. Connect compatible mechanical ports explicitly
when intentional interpenetration is part of the assembly.

This is a **conservative display-envelope check**, not a CAD interference or
physics contact test. It uses world-axis-aligned boxes around each leaf's
`appearance.shape`, or its shared family default; cylinders and spheres use
bounding boxes. Rotated shapes can therefore reject a placement whose detailed
meshes would clear. Catalog meshes are not used as collision geometry. Author
an appropriate display shape when the default envelope is unsuitable.

The shared `SystemStore` checks placement/addition/replacement/appearance
transactions under the file lock, before saving or recording history. This
covers both viewers, REST `system_move`, and raw `system` command batches.
The in-memory `sim_system::apply` remains a construction primitive for building
unlaid-out models; callers that don't use the store can call
`display_overlap::preview` / `check` explicitly.

## REST

Send commands to `POST /v1/commands` as `{"command":"…","args":{…}}`.
It returns a job receipt; poll the returned job via `/v1/jobs/<id>` until it
finishes and inspect its result. `system_state` reports the revision, local
placements, grid, discussions and the display-only semantics.

Read the grid:

```json
{"command":"system_grid","args":{}}
```

Set the grid (optional `expected_revision` rejects stale writes):

```json
{"command":"system_grid","args":{"grid":{"visible":true,"snap":true,"spacing_m":0.01,"origin_m":[0,0,0],"plane":"xz"},"expected_revision":3}}
```

Preview placement, using exactly the same quantization as mouse dragging:

```json
{"command":"system_move","args":{"names":["motor","gearbox"],"position_m":[0.041,0,0.022],"snap":true,"preview":true,"expected_revision":4}}
```

The first name is the anchor. Other names keep their offsets. The response
contains resolved `move_instance` commands, revision, frame, units,
display-only semantics, and an `overlap` report. An invalid preview succeeds
with `overlap.allowed:false` and `conflicts` containing both part paths,
world bounds and penetration in metres. Commit rejects that same placement.
`system_state.overlap_policy` documents the assumptions. Set `preview:false` to commit. `snap:false` means
exact coordinates. Existing `system` command batches can also apply exact
`move_instance` edits. `system_undo`/`system_redo` work across editors.

## Operate the visible UI through REST

`system_ui` drives the same handlers as the physical builder's buttons and
component clicks. It does not synthesize desktop mouse/keyboard events.
Use this for interaction and `system_discussions` for direct persisted data.

Discover the controls currently displayed:

```json
{"command":"system_ui","args":{"action":{"operation":"controls"}}}
```

The result includes `ready`, `ui_revision`, and `controls` with `id`, `label`,
`enabled`, and a descriptive `action`. If `ready:false`, request controls again
after the pending UI rebuild. Then activate a returned control:

```json
{"command":"system_ui","args":{"action":{"operation":"activate","id":"control-ID","ui_revision":1790000000000001}}}
```

IDs are opaque. Use the revision from that response; a stale generation or a
disabled control is rejected. This protects palette indices and port suggestions
when the level, selection, filter, document or displayed controls change.
Examples include More, Rename, Resolve, Reply, parameter editors, grouping,
reference controls, palette cards, run controls and undo. Actions that open a
text field expose its purpose and current text in `system_state.ui.draft`.

Stable semantic operations are also available:

```json
{"command":"system_ui","args":{"action":{"operation":"click_part","component":"motor","add":false}}}
```

```json
{"command":"system_ui","args":{"action":{"operation":"open_thread","id":"thread-ID"}}}
```

```json
{"command":"system_ui","args":{"action":{"operation":"annotate","target":"motor","pin_m":[0.002,0.01,0]}}}
```

`click_part` has the same selection/connection behavior as clicking the rendered
part in the current mode. In Annotate mode, optional `point_m` is a local pin
offset. `annotate` opens a new note draft on any existing part/group at a local
point, as the Annotate tool does; `open_thread` opens the Notes panel exactly
like a marker click. Pins and positions are **display-only metres**.

Write or submit any open field without typing on the desktop:

```json
{"command":"system_ui","args":{"action":{"operation":"input","text":"Check the shaft clearance.","expected_text":"","submit":true}}}
```

`expected_text` must match the draft exactly. `submit:false` only updates the
visible draft; `submit:true` invokes its normal submit handler. Validation
errors retain the text. `cancel_input` also requires `expected_text`. These
are explicit draft edits; ordinary REST discussion CRUD still preserves drafts.

Other operations are `tab` (`library`, `discussions`, `outline`, `studies`,
`references`) and `mode` (`select`, `connect`, `annotate`). Each request can
carry `expected_revision` next to `action`. Mutating operations return updated
builder state; the next render may still be pending. `controls_ready` tells
clients when newly displayed controls can be discovered. No visible UI action
bypasses shared model validation, overlap rejection or undo.

## Discussions

Choose **Annotate** in the toolbar (or press **N**), then click a rendered
surface. A `+` pin marks the pending comment at that exact point. The Notes
panel opens for writing; Enter posts and Escape cancels. The stored offset is
local to the clicked part, so the pin follows its display transform.

Numbered, constant-size pins with leader lines remain visible beside the
rendered model as the camera moves. Every linked part/group gets a marker;
nearby markers are separated to keep their click targets usable. Hover a pin
to see the title, target and comment count and highlight the associated parts.
**Click a pin to open that discussion in the Notes panel**, then use the reply box to join
it. Pin clicks use UI hit testing so they take priority over dragging the part
beneath them. Hidden/missing targets and resolved discussions have no viewport
pins. REST-created discussions get markers too, including when `pin_m` is
omitted (the target origin is used). `system_state.annotation_markers` reports
the thread, target, number and window-local logical pixel positions.

The Notes tab starts with a compact list of conversations. Opening a note
shows its messages and a fixed reply box; Post reply or Enter sends the draft.
More contains Rename, Add selected parts, Resolve, and Delete. Each message's
ellipsis menu contains Edit and Delete. All notes returns to the list.

For a group or multi-part note, select parts and use Notes → More →
Note on selected parts. Change your name is also in this menu. Write the
comment and press Enter or Post note to save. Shift+Enter adds a line; Escape cancels. Replies, edits, resolution,
titles, deletion and linking the current selection use the shared undo history.
Filters show open threads or threads related to the selection. The schematic
builder also exposes the shared threads and basic comment editing.

Links can be inserted as `[motor](part:motor)` or
`[drive](group:drive)`. The physical builder resolves these into persistent
links when saving. Target chips highlight on hover and open linked-part
inspection on click. Context restores the thread's captured view; Parts
isolates its targets; Return to assembly restores the previous camera,
visibility and selection. The explicit pin offset is local to the first target; additional linked targets use their origins. Click a pin to open its thread. REST can set an exact
local pin offset; More → Reset marker to part origin resets it.

Threads are stored in the system document's `discussions` field. This lets
grouping and annotation reference updates share one atomic edit and undo.
`display_id` values persist across rename and regroup operations; repeated
definition occurrences are distinguished using ancestor identities. A deleted
or ambiguous target is retained as `missing`; a replacement with the same name
does not silently inherit its comments. Timestamps are Unix seconds in UTC.
Notes and grid changes do not require restarting the physical viewer's run.

Existing `*.annotations.json` sidecars are preserved. **More → Import existing notes**
copies resolvable notes into threads without deleting the originals. Saved
views and original links remain available through the existing `annotations`
API. Importing the same note again does not duplicate it.

The physical viewer exposes `system_discussions`:

```json
{"command":"system_discussions","args":{"action":{"operation":"create","title":"Check drive clearance","targets":["motor","gearbox"],"body":"Check [motor](part:motor) against the frame.","author":"User","pin_m":[0,0.01,0]}}}
```

```json
{"command":"system_discussions","args":{"action":{"operation":"reply","id":"thread-ID","body":"The display has been rearranged; physical geometry is unchanged.","author":"Codex","links":["motor"]},"expected_revision":5}}
```

Operations:

| Operation | Arguments |
|---|---|
| `list` | Optional `target` path, `resolved` boolean, `author` |
| `get` | `id` |
| `create` | `title`, `targets`, `body`, `author`, optional `pin_m` |
| `reply` | `id`, `body`, `author`, optional `links` |
| `edit_comment` | `id`, `comment` ID, `body` |
| `delete_comment` | `id`, `comment` ID |
| `resolve` | `id`, `resolved` |
| `delete` | `id` |
| `title` | `id`, `title` |
| `link` | `id`, `targets` to append |
| `pin` | `id`, `pin_m` (null resets to target origin) |
| `show` | `id`, `mode`: `context`, `parts` or `highlight` |
| `highlight` | `targets` (empty clears emphasis) |
| `inspect_target` | `target` path |
| `back` | No arguments |
| `import_legacy` | No arguments |

All can carry `expected_revision` next to `action`. Reading or appending through
REST never replaces an unsent UI draft. Both viewers and the CLI can edit the
same persisted data through shared `put_thread`, `add_comment`, `delete_thread`
and `set_display_grid` system commands.

## Codex in annotations

The physical viewer's Notes panel has **Ask Codex** for each discussion and an
**Auto-answer** toggle in the notes list. Automatic answers are off initially.
Enabling them baselines existing comments and watches new posted human comments,
including those added through REST or another editor. Editing an old comment
alone does not launch a new turn. Replies authored by Codex (or carrying an
agent run ID) never trigger another run. New follow-ups are queued in order.
One serial worker keeps the interface responsive and preserves a resumable
Codex conversation per discussion.

The requested model is `gpt-6-astra`, reasoning `high`. The local Codex CLI must
be logged in and expose that combination via `model/list`; an unavailable model
produces a visible error instead of a silent substitution. The viewer uses
`SIM_CODEX_EXECUTABLE`, then `~/.local/bin/codex` if present, then `codex` on PATH.
Launch the viewer from the project repository: its current directory (or nearest
Git ancestor) is the agent's working root. The current assembly snapshot, source
file/revision, discussion, persistent part identities, and REST resource URLs
accompany each question. Codex can inspect additional repository files. The child client
keeps filesystem access read-only and enables network access for viewer REST
inspection; agent instructions permit read-only inspection commands only.

Every new agent turn also receives `sim.model-context/v1`, prepared on a worker
from that exact document snapshot by the reusable `sim-model-context` library:

- Linked parts/groups and comment links, stable identity lineage, group contents,
  parent definitions, JSON source pointers and imported library file/hash references.
- Actual resolved component parameters through the shared model factory, plus
  authored bindings (including inherited overrides), units, provenance and uncertainty.
  Registry defaults are labeled as defaults; typical example values are not substituted.
- Typed ports, quantity/connector definitions, complete merged nets across subsystem
  boundaries, and one connection hop of neighboring components.
- Registry explanations, equations, parameter help, limits and tradeoffs, available
  observable definitions, model findings, saved studies and run/profile settings.
- Display placements and shapes explicitly labeled **display-only**, current viewer
  compile status and authored part-library paths/load errors for further investigation.

This is model inspection, not a simulation run or evidence of physical calibration.
If resolution fails, Codex receives the authored context and the error, not a stale
model substituted as current. Live measurement values are not manufactured or copied
into this packet: `/v1/description` and `/v1/measurements` carry their own identities
and revisions, which must be checked against the current model and compile status.
Missing physical metadata stays unknown. Deleted annotation targets remain missing,
even if a new part reuses the path.

Focused expansion includes up to 128 instances, prioritizing annotation targets and
their descendants before neighbors. The response explicitly reports omitted counts;
complete net terminal lists and source definitions remain available. Codex can inspect
another region with the same read-only REST operation:

```json
{"command":"system_context","args":{"discussion":"thread-ID"}}
{"command":"system_context","args":{"targets":["gearbox","motor"]}}
```

POST to `/v1/commands`, then GET the returned job URL as usual. Empty args inspect
the model. Inspection runs off the UI thread and does not select parts, move the
camera, edit the document or add undo entries. The notes card and `/v1/agent` expose
a compact `context_summary` with source hash/revision, scope and resolution status.
This summary survives reply delivery; subsequent inspections reflect the current
document rather than reconstructing a past answer's full packet. Large agent contexts
over 1 MB fail visibly and can be narrowed instead of being silently truncated.

Comment bodies remain Markdown in the document and REST API. The native panel
renders headings, emphasis, lists and monospaced code, with compact citation
buttons instead of raw link destinations. Local file citations open a read-only
source excerpt in the right inspector, at the requested line; **Back to inspector**
restores the previous panel. Source reads run in the background and stay within
the project root. `system_state.reference` exposes loading/result/error state,
and citation buttons are discoverable through `system_ui/controls`. HTTP(S)
references open the browser when activated. HTML is displayed as inert text.

The activity card displays actual lifecycle, tool and shared progress events.
The main status is concise; the disclosure shows three recent bounded entries,
with word/character wrapping and clipping. Full retained activity remains in REST.
**Stop**, **Retry**, and **Show activity** use the same handlers as REST. Pins and
note cards display an activity/unread indicator; opening a discussion marks its
reply read without moving the camera. The completed answer is a normal Codex
comment with part/group links, hover effects and shared undo. Replies preserve
unfinished drafts. Links bind to the source identities and follow later renames;
an unknown generated link rejects delivery rather than pointing to a different
part. If the source changes during work, `source_revision` identifies the
snapshot used. Answers are not automatically recomputed on unrelated edits.

Answer mode reads and explains; it does not implement code or assembly changes.
Display placement, agent status, and annotations are not physics inputs. The
local supervisor uses the existing Codex login and remote model service; this
is not an offline model. No extra API key is stored in the system document.

### REST agent controls and streaming

Submit commands to `POST /v1/commands`, then poll the returned HTTP job URL as
usual. That job acknowledges the control operation. The returned `run` ID tracks
the longer Codex task through `system_agent/status` or `GET /v1/agent`.

```json
{"command":"system_agent","args":{"action":{"operation":"ask","discussion":"thread-ID","question":"Why was this transmission chosen?","request_id":"gearbox-question-1"}}}
```

`question` is optional (uses the latest human comment). Supply a stable
`request_id` to deduplicate client retries. A different question using that same
ID is rejected. Follow up using another `ask` on the same discussion with a new
request ID; the saved Codex conversation is resumed.

| Operation | Arguments | Effect |
|---|---|---|
| `status` | none | State, runs, model/effort, retained activity and cursor |
| `configure` | `auto_answer` | Persist automatic-answer preference |
| `ask` | `discussion`, optional `question`, `request_id` | Queue an answer/follow-up |
| `cancel` | `run` | Cancel queued work or interrupt the active turn |
| `retry` | `run` | Retry failed/cancelled work in its saved conversation |
| `mark_read` | `discussion` | Clear unread indicators |
| `activity` | none | Toggle the visible expanded activity card |

```sh
curl -N http://127.0.0.1:8437/v1/events/agent
```

The SSE stream emits `snapshot` events on changes, including retained activity
entries with monotonically increasing `sequence` and an `event_cursor`. Track
that cursor to avoid displaying repeated activity. Each connection begins with
a full snapshot; compare `oldest_event` with the last cursor to detect an event
retention gap. Streams send heartbeats and close after 20 seconds; reconnect
(the advertised retry is 500 ms). At most two concurrent streams reserve the
remaining HTTP workers for commands. Existing loopback/Host/origin restrictions
also apply to streams. `GET /v1/agent` supports polling instead.

State is local to the system file in `.<filename>.agents/state.json`, excluded
from Git. It contains conversation IDs, pending context, source revisions,
activity and replies. One viewer owns the agent service for a file at a time;
another viewer can still edit the shared document but cannot duplicate agent
work. Closing the viewer stops its supervisor; automatic responses resume when
that file is reopened. A crashed active turn becomes a visible retryable failure.
Completed-but-unattached replies recover using the same comment ID, so attachment
is idempotent. Agent progress does not add entries to system undo history.

## Verification

`cargo test -p sim-system --test display` checks that snapped moves preserve
relative offsets and leave the compiled physics model byte-equivalent, local
grids work in rotated groups, references survive rename/group/ungroup, deleted
targets do not attach to reused names, persistence works, stale edits fail,
and shared undo/redo restores discussions. It also checks overlap rejection,
atomic file/history preservation, touching, repair of existing overlap,
selection translation, rotated nested bounds, mechanical mates, and the
absence of an electrical-wiring exemption. The `sim-spatial` library test
checks that REST comments preserve UI drafts and reject conflicting edits.
`cargo test -p sim-spatial --lib` also covers local surface anchors, opening a
thread through the pin action, pin separation, discovery of actual ECS controls,
disabled/stale control rejection, and REST draft compare-and-swap.

`cargo test -p sim-agent -p sim-api` checks the supervisor lifecycle and real HTTP
stream delivery while ordinary REST reads remain responsive. The builder agent
test checks single reply attachment, identity-following links after a rename,
invalid-link rejection, draft preservation, and shared undo.

### Responsive dragging

Mesh drags, axis handles, grid snapping and Alt-to-bypass snapping update the
visual preview every Bevy frame. Parts, part/group annotation anchors and handles
use the same current preview; source placement stays unchanged until release.
Overlap validation runs on a worker with a single latest-position mailbox. Older
pointer positions are replaced, not queued for playback, and stale validation
results cannot approve the current position. Unknown validation remains pending.

On release the preview stays visible while the shared store transaction validates
and saves in the background. The transaction checks the original revision and
full overlap policy again, and creates one undo entry. The preview is retired only
when the replacement scene arrives. Invalid/stale drops restore the saved model;
Escape and release outside the viewport cancel before committing. A transaction
already being saved completes atomically. Filesystem polling and picked-entity
replacement are deferred during dragging. Display placement still does not change
CAD geometry or physical simulation inputs.

Regression tests cover actual Bevy drag observers, same-frame mesh/pin motion,
all grid planes and axes, grab offsets, rotated frames, multiselection/group
rigidity, degenerate rays, cancellation, stale validation, authoritative overlap
and revision checks, undo, REST preview parity and drop-to-scene continuity.
The CPU acceptance case exercises 1,024 Bevy transforms and 100 annotation anchors
for 240 measured frames while validation is blocked: p99 must remain below
16.667 ms. This is a CPU interaction budget, not a GPU or whole-application FPS claim.
