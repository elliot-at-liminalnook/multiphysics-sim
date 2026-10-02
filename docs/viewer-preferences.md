# Native viewer preferences (T45)

`app::settings::SettingsOwner` owns preference loading, legacy migration,
validated projections, readiness, dirty revisions, serialized publication,
diagnostics, retry and best-effort shutdown. T45 is verified by source reading
only: no builds, fixtures, launches, hardware operations or parity runs executed.

Launch remains `cargo run -p sim-spatial -- path/to/model.rcad` or
`--cad-url http://127.0.0.1:8420`; these are instructions, not execution receipts.
Python/OCCT remains the CAD authority. A restart restores tool/form choices,
never geometry, measured calibration or active hardware operation.

| Stored field | Preference meaning and compatibility | Excluded source or active intent |
|---|---|---|
| recents.version, modes | Legacy version 1; mode names retain newest-first entries, de-duplicate known document identity, KEEP=10 | No source document content or unsaved edits |
| entry.document.path | Absolute/canonical accepted path; canonicalization stays on jobs | No geometry import or automatic reopen |
| entry.document.preset | Existing named preset identity | No controller start |
| entry.document.url | Existing accepted service URL | No connection/activation on load |
| entry.opened | Original Unix timestamp, retained through migration | No physical provenance |
| hardware.version | Existing version 1; validated before publication | No calibration schema |
| calibration.drive_mode | Optional inactive calibration form choice | No drive command or running drive |
| calibration.hold_others | Optional inactive form choice | No hold operation or operator confirmation |
| mirror.enabled | Display preference; existing default true | No hardware command |
| mirror.leg | Existing display leg choice, default +X | No source topology change |
| mirror.bindings[motor].joint | Existing CAD joint reference; incomplete binding uses Hip servo output | No CAD physical definition |
| mirror.bindings[motor].polarity | Sign normalized negative→−1, otherwise +1 | No measured actuator model |
| mirror.bindings[motor].align | Home/mid CAD pose reference; missing value follows joint default | No measured alignment angle or taught travel window |
| sync.leg | Optional retained inactive mapping leg | No armed synchronization |
| sync.amplitude | Optional bench form scale 0.03/0.05/0.09 | No active motion or raised physical limit |
| sync.bindings[].coordinate | Ordered existing configuration coordinate reference; missing string remains empty | No synthesized CAD joint |
| sync.bindings[].motor_id | Stored mapping ID; missing 0 retains existing mapping fallback semantics | No motor drive |
| sync.bindings[].polarity | Normalized ±1, missing +1 | No calibrated polarity measurement |
| cad.wall_threshold | Optional remembered mm threshold; finite 0.1..20; absent form starts 1.2 | No wall geometry or check result |
| cad.fastener.size | M2/M2.5/M3/M4/M5/M6/M8; default M3 | No source fastener hole |
| cad.fastener.kind | clearance/tap/counterbore/countersink/insert; default clearance | No source edit or undo entry |
| cad.fastener.extra | Finite 0..1 mm, default 0 | No measured manufacturing fit |
| cad.fastener.depth | Finite 0..500 mm, 0 means through | No source depth |
| cad.clearance | Finite −5..5 mm, default 0.2 | No geometry offset |

Picks, document identity, source revision, results, caches, jobs, connections,
operator confirmations, jog state, drive/sync arming, controllers, measured
models, calibration records, taught travel windows and server/CAD physical data
are never preferences. Inactive hardware choices still pass existing connection
and configuration validation before explicit activation. STOP and release/loss
handling retain their independent safety paths.

## File and migration contract

The unified file is `viewer-preferences.json` in the existing recents config
location: nonempty `SIM_SPATIAL_CONFIG_DIR`, then nonempty
`XDG_CONFIG_HOME/sim-spatial`, then macOS
`HOME/Library/Application Support/sim-spatial` or other platforms'
`HOME/.config/sim-spatial`. Without an available home/config location, the jobs owner still loads the exact legacy hardware source for
session-only forms. Diagnostics report the missing location; publication stays
blocked and dirty work remains unsaved. `settings_retry` does not turn a
successfully loaded session-only configuration into a durable one; restart with
a config location to persist.

The exact hardware legacy import path remains nonempty
`SIM_SPATIAL_PREFERENCES`, else `HOME/.config/sim-spatial/hardware-preferences.json`.
Its existing no-HOME behavior remains relative `.config/sim-spatial/...`, resolved
by the jobs owner; macOS/XDG fallback searches are not added. The hardware override
identifies the import source, not a competing writer destination.

A valid unified schema-1 file wins over both legacy files. Without it, import
`recent.json` at the recents config directory and the exact hardware path; absent
legacy files use initial defaults. There were no durable legacy CAD print defaults.
Original legacy files are never deleted, renamed or overwritten. The unified file
records `hardware_source`: changing the hardware override to a different source
while reusing that unified file refuses loading/saving, so values cannot disappear
behind another import. Restore the original override or choose a separate
`SIM_SPATIAL_CONFIG_DIR`. This refusal also covers relative/absolute source identity
changes; it is deliberate rather than an expanded fallback search.

Migration is idempotent: subsequent startup reads the unified snapshot and does
not re-import changed legacy files. Unknown object data, nested binding metadata,
unknown modes and unknown recent variants remain raw alongside the validated
projections; unrecognized recent variants are preserved but not offered as
openable documents. Later saves overlay known fields without intentionally
resetting unknown metadata. Active mirror bindings are rebuilt exclusively from
current motor keys; retained bindings merge unknown metadata by that motor identity.
Sync row metadata follows coordinate identity when named (index fallback for
incomplete rows). Duplicate sync identities match in occurrence order, with each
current row consuming at most one prior row; unmatched prior occurrences are
archived. Recent metadata follows parsed document identity. Omitted binding
rows remain absent from active saved and reloaded projections.

`preserved_source` remains the immutable initial raw evidence. Removed rows observed
in later loaded snapshots are also retained in `retained_rows`, a flat version-1
archive of `{collection, identity, value}` records. Collections are `mirror` and
`sync`; values are complete removed raw rows, not enclosing snapshots or archives.
Identical records are deduplicated; distinct historical row values remain recoverable.
The archive is never interpreted as active settings or merged back into bindings.
This preserves newly encountered metadata after a later startup without changing
the original migration evidence or recursively nesting earlier archives.

The archive is bounded to 256 records and 1 MiB of encoded JSON. Malformed/newer
archives or an addition exceeding either bound return a named snapshot error;
publication does not proceed and dirty state remains available for retry. No record
is evicted or truncated to make a save succeed. Recovery requires preserving and
reviewing the input/archive and choosing a separate config location or deliberately
managing the archived data outside this automatic path. Unknown fields are never
executable intent.

Newer schemas, malformed JSON, malformed known values and unreadable inputs block
publication. They remain visible diagnostics; dirty state stays retained for retry.
The owner does not replace a protected input with an apparently successful default
snapshot. Recovery requires correcting permissions/content or selecting a separate
config location and explicitly retrying/restarting; preserve the original first.
Retry never requires an unrelated preference edit. No automatic recovery deletes
or overwrites the original migration input.

## Pinned settings integration

The package is `bevy-settings =0.19.1`, Rust module `bevy_settings`.
[Official crate documentation](https://docs.rs/bevy-settings/0.19.1/bevy_settings/),
[SettingsGroup](https://docs.rs/bevy-settings/0.19.1/bevy_settings/trait.SettingsGroup.html)
and [SettingsPlugin](https://docs.rs/bevy-settings/0.19.1/bevy_settings/struct.SettingsPlugin.html)
are authoritative alongside downloaded pinned source. The derive is in
`bevy_ecs_macros-0.19.1/src/lib.rs`, including manifest resolution for the
hyphenated package. `PreferenceGroup` derives Resource, Reflect and SettingsGroup,
registers reflected resource/default/group type data and uses the actual named
source/group contract for the envelope. The owner publishes validated JSON strings
to that registered resource; registration is used rather than a decorative derive.

The stock plugin immediately loads its source while building the app. Stock save
commands bypass jobs through `IoTaskPool::scope`; pinned task-pool source waits for
scope results, so the name save_async does not promise nonblocking caller behavior.
Stock change ticks advance even after logged store errors and do not establish
completed durable publication. The narrow seam retains the pinned registration
and file/group contracts while `jobs` is the only disk-work owner. Stock
SettingsPlugin and its save commands are not installed; there is no second writer.
Source reading: `bevy-settings-0.19.1/src/lib.rs:96` builds and immediately
loads; `:139` declares the three SettingsGroup methods; `:165` registers resource
type data; `:205`, `:223`, `:243` define save commands; `:265` advances ticks
without a publication Result; `:381` scans reflection; `store_fs.rs:17` selects
OS preferences paths, `:64` uses scoped task-pool I/O and `:111` conflates read
failure with absence. `bevy_ecs_macros-0.19.1/src/lib.rs:531` resolves the
hyphenated manifest name, and `:632` implements the derive.

The custom schema envelope enables non-destructive legacy migration and raw unknown
preservation not supplied by stock loading.

## Source reading and lifecycle

1. Startup installs settings without synchronously reading user files. An Io job
   resolves paths and loads/validates the unified file or migration inputs.
2. Picker may open before readiness: discovery consumes owner recents and waits
   for/refreshes after publication; it has no independent preference disk backend.
   Accepted opens queue timestamped records; jobs canonicalizes filesystem paths.
3. Hardware adapters seed inactive form choices. An intervening form edit owns
   that field/subgroup, so a late startup result cannot overwrite it.
   CAD tracks wall threshold, fastener defaults and clearance independently; even
   an accepted equal-value choice claims its field against late loading. Hardware
   form choices use field/subgroup ownership, with binding rows treated as a mapping
   group. Early active motion conservatively claims the entire hardware group to
   prevent loading from changing its configuration. Untouched saved fields still
   load. Recents merge accepted queued records into the loaded base. Loading
   sends only the existing host-only `LinkCommand::Inputs` assignment when a
   calibration link is already connected, through `handlers::inputs_changed`.
   Thus a connection completed before loading receives the same displayed
   drive-mode/hold-others choices before a later explicit Select/jog command.
   The session Inputs branch performs no hardware request, preserves the speed-reset
   guard and restores no selected motor, held keys, motion intent or confirmation.
   Loading does not issue physical hardware requests or restore activation.
4. CAD `actions::Cx` carries the owner; `ops::Env` seeds forms and closed fastener
   picks from it, and snapshot `Parts` reports those same defaults. Existing drafts
   stay intact if loading finishes after their opening. A validated wall launch or
   accepted revision-guarded edit dispatch updates global defaults. Rejection/stale
   revision leaves defaults unchanged. Replacement discards local picks/results,
   while the global defaults remain. Geometry and undo continue on the CAD service.
5. Dirty snapshots carry revisions. One save runs at a time during normal operation;
   the shared publication gate prevents a delayed older snapshot overwriting a newer
   shutdown snapshot. Successful publication includes file synchronization, atomic
   rename and directory synchronization. A post-rename durability error remains an
   error, not a successful receipt. Failure retains diagnostic and dirty revision;
   timed retry (five seconds) or explicit `settings_retry` works without another
   preference edit. `settings_status` reports readiness, dirty/saved revisions,
   blocked state, diagnostics and in-flight saving. Retry of failed startup reloads
   the protected input; retry of dirty ready preferences resubmits publication.
6. Next startup reads the successfully published unified snapshot, preserving unknown
   data and bypassing legacy import. Loaded groups are merged with explicit mutation
   ownership, rather than registration order or resource change ticks.

Normal frame work uses public ViewerSet Input → Actions → JobResults → SimSync →
Present. Readiness/results and retry are ordered through public settings sets.
Pending work is resource state, not an expiring mode-gated message. File-byte serialization
and disk publication belong to jobs; small validated projection/envelope encoding
happens in frame systems before handing owned snapshots to jobs; transient UI entities own no durable settings.

Shutdown submits the latest dirty snapshot and still-queued accepted recents
without synchronous UI I/O. The final job canonicalizes queued records and
publishes through the same gate, serialized against existing work. It is best effort: dropping a handle
alone is not guaranteed durability; even an ordinary immediate process exit can
end before the final job finishes. Abrupt exit can lose dirty preferences or
interrupt startup/migration before publication. CAD unsaved-edit guards and hardware
STOP/loss handling remain unchanged. Later authorized execution must verify the
isolated migration, precedence/override, unknown/future/corrupt inputs, late-load,
picker, hardware non-activation, CAD validation/replacement, save ordering,
failure/retry/publication and shutdown fixtures. None have been executed in T45.

Publication compares the current unified JSON with the last loaded or published
snapshot while holding the serialization gate. A changed, unreadable/corrupt or
future-schema file is preserved and refused: restart after reviewing the input.
This detects changes present at the prepublication check; it is not an OS lock
against another process writing between comparison and rename. Run one viewer
owner per config location. Post-rename directory-sync errors retain dirty state
and recognize our bytes as the expected retry base without acknowledging success.
Shutdown includes queued recent normalization plus current hardware/CAD defaults.
Accepted recent intent advances the dirty revision before canonicalization and
`settings_status` reports pending records separately from disk publication.

## Batch evidence (source reading only)

| Checklist ID | Source evidence; execution remains unverified |
|---|---|
| persisted-settings:task-T45.1 | `app/settings/{mod,plugin,jobs}.rs`: reflected group/source contracts, validated typed owner, exact migration paths, protected raw input and one jobs disk path |
| persisted-settings:task-T45.2 | `app/recent.rs`, `app/switch/mod.rs`, `app/picker/`, `robot/hardware/{actions,settings,mirror,sync}.rs`: adapters no longer load/save files |
| persisted-settings:task-T45.3 | `cad/{actions,ops,snapshot}`, `cad/print/`: global default forms/state and accepted guarded dispatch; this guide and architecture record lifecycle and unexecuted status |
| persisted-settings:outcome-1 | `settings/plugin.rs::{start,tick,land_load,land_save}`, `settings/actions.rs`: readiness, dirty revision acknowledgment, status/retry, serialized publication and nonblocking shutdown |
| persisted-settings:outcome-2 | `settings/jobs.rs::{load,known_document,snapshot,publish_ordered}`: legacy precedence, path identity, raw unknown preservation and fail-closed input/publication |
| persisted-settings:outcome-3 | `print/checks.rs::send`, `print/edits.rs::{send,seed}`, `ops/mod.rs::Cx::env`, `snapshot.rs::Parts::defaults`: same validated defaults across replacement and restart |
| persisted-settings:outcome-4 | `settings/tests.rs`, `picker/settings_tests.rs`, `hardware/actions/tests.rs`, `hardware/settings.rs` fixtures, `print/{checks_tests,edits_tests}.rs`: isolated lifecycle fixtures, all unexecuted |

Library-test plugin startup deliberately performs no environment-based disk load;
disk fixtures call the shared jobs helpers with fresh isolated paths. Path precedence
fixtures use pure resolvers rather than changing process-wide environment variables.
Shutdown failures can only be logged while the process remains alive; resource teardown
cannot retain live diagnostics after destruction or guarantee completion on abrupt exit.

## T45 repair lifecycle (source reading only)

The connection-before-load fixture exercises the existing link/session path:
connection installs host defaults, validated late preferences publish the form,
publication queues host Inputs, and only a subsequent explicit Select/jog requests
hardware through the existing link/session command path. Its fake request log distinguishes
host assignment from physical requests. No fixture was executed.

Persistence fixtures save and reload removed mirror keys, retained metadata by
identity, and removal after a later startup adds new unknown row metadata to a file
already containing `preserved_source`. The flat archive preserves that metadata
while both active collections omit the removed rows. Written bound/schema fixtures
retain fail-closed publication; all execution remains unverified. These repairs
retain T45.1–T45.3 and all four outcome IDs above, the one jobs owner, existing CAD
validation/undo, recents and hardware STOP/release/explicit activation.
