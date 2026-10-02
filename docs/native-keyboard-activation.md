# Native activation — T49 source evidence

This bounded inventory covers native-keyboard-activation T49.1–T49.3. It is
implementation and source-review evidence only. Fixtures are written and
inspected, **unexecuted**; compilation, interactive layout and GUI parity are
unverified. No viewer, build, test, screenshot, export or hardware operation was
run. Python/OCCT, browser hardware/calibration references and unqualified CAD
migration phases remain available. The native launch remains `sim-spatial` with
its existing mode/document flags; no second viewer or execution path was added.

## Ownership and schedule

`ui_kit/activation.rs` owns the ordinary input contract. A rendered entity owns
`Ordinary`, its existing typed action component, navigation index, enabled
projection, outline, immutable rendered source stamp and transient `Activated`.
The marker records an occurrence on the original generational entity: it never
resolves a new row using an old index. Feature converters remain the writers of
`Act<A>`; the existing action apply systems remain the validated owners.
Persistent switcher/close controls are window intent, with no physical source.
Document registry and CAD/Study/Builder retain durable sources and revisions.

The existing `InputFocus` is the sole focus resource. Modal return focus and
pointer-cause authorization are small global window state; neither owns edits.
TextField entities retain drafts, caret/selection and source/intent anchors even
when rendered children are replaced. A unique matching InputIdentity and source
rebinds the anchor; ambiguity, hiding, source replacement or disappearance clears
focus and emits Blur without deleting the draft. No positional reconstruction is
used for retained editor identity. Path identity uses its stable label; Study
identity uses the retained Study and Field, excluding text/revision. Composer
identity retains thread/edit/reply purpose without mutable draft text. CAD editor
identities additionally retain source, form, numeric and rename lifetimes.

PreUpdate orders InputSystems → eligibility (before picking Hover) → modal
containment → TextInputSet (after picking Last and UI Focus) → pinned focused
Dispatch → shortcut consumption. Deferred activation capture is visible before
Update. Update orders REST → focus-input capture → ActivationSet::Validate →
InputSet::Window conversion → occurrence cleanup → authoritative Actions.
Feature presentation remains in Present; source stamps land in PostUpdate after
rendering. Deferred command edges flush capture/refusal before conversion.
Bevy's mode-scoped linked despawn and generational entity identity refuse removed
controls. No expensive work, worker, physics or durable queue was added.

Pinned DefaultPlugins already installs InputFocusPlugin, InputDispatchPlugin and
UiWidgetsPlugins/ButtonPlugin. The kit checks ButtonPlugin installation for
windowless fixtures and installs TabNavigationPlugin once. Startup attaches its
window observer to PrimaryWindow. Kit roots receive nonmodal TabGroup; backdrops
and custom CAD operation forms have ModalFocus/TabGroup::modal and initialize
inside the group. Pinned navigation only gathers TabIndex: it does **not** filter
hidden/disabled controls, so kit eligibility removes indexes and synchronizes
InteractionDisabled with Enabled and ancestor Display/Visibility before dispatch.

## Pinned signatures inspected

Registry source `bevy_ui_widgets-0.19.1/src/button.rs`:
`Button`, `ActivateOnPress`, `ButtonPlugin`; keyboard observer reads
`On<FocusedInput<KeyboardInput>>`, rejects repeat, consumes Enter/Space and
queues `Activate { entity }`. Pointer press queues activation before release.
The pointer widget does not filter Primary: the kit records pointer cause before
deferred Activate, refusing secondary/middle presses. Explicit primary focus
uses InputFocus::set(entity, FocusCause::Pressed). This preserves the previous
left/touch-only ordinary behavior. HeldControl strips generic widget behavior;
KeyboardOnly is reserved for compound tree selection, never hardware holds.

`bevy_input_focus-0.19.1/src/lib.rs`: InputFocus::get/set/clear,
InputFocusVisible, FocusedInput, AcquireFocus, InputDispatchPlugin,
InputFocusSystems::Dispatch. Default feature resolution enables picking; pinned
TabNavigationPlugin also installs AcquireFocus/click_to_focus observers.
`tab_navigation.rs`: TabIndex(i32), TabGroup::new/modal,
TabNavigation::navigate/initialize and NavAction; modal containment only applies
once focus is inside, so the kit owns initial/return focus. `bevy_ui` Outline::new
provides a focus ring independently of hover border styling. Text input consumes
activation keys through its existing one writer; repeated Enter/Tab/Escape do not
resubmit. Default editor Tab delegates through the retained rendered anchor;
indent fields retain indentation. Releases remain observable.

## Bounded family inventory

Paths below are relative to `crates/sim-spatial/src`. Ordinary adapters now read
`Activated`; remaining Changed<Interaction> usage is styling or named gestures.

| Family / former adapter | Rendered source and new conversion | Existing authority / identity |
|---|---|---|
| Switcher `switcher_clicks` | `app/switcher.rs` kit segments | WindowAction::Switch → `app/switch`; `mode:*`, viewer_mode unchanged |
| Picker `clicks` | `app/picker/mod.rs`, kit picker/path rows; rendered picker revision refused before positional choice lookup | WindowAction::Switch; picker:* identities unchanged |
| Close `clicks` | `app/close/ui.rs` kit actual controls | CloseAction → CloseOwner; close:request/retry/cancel/preference-loss identities unchanged |
| Build `buttons` | `builder/actions.rs`, all toolbar/library/inspector/discussion/gait controls; raw markers/schematic controls marked Ordinary | SystemAction::RenderedUi retains path/revision/level/palette and DocumentId, checks before existing dispatch; REST/system_ui keep existing commands |
| Retained Study forms | `builder/calibration/study/forms.rs`, `ui.rs` fields and refinement submissions | stamped StudyAction/retained owner; stale focus stamp refused; same registry/shared refinement validation |
| Robot and hardware ordinary controls | `robot/actions/keys.rs`, `panel_ui.rs`, hardware/actions/input.rs; raw scene links marked | existing RobotAction/HardwareAction owners; SelectLink validates index and stable name; remote motion refusal unchanged |
| Lessons | `lesson/actions.rs`, outline/practice/cards/scene_card/ui raw rows | LessonCommand owner; hover highlight kept separate; existing shared fields |
| Phenomena | `phenomena/panel.rs` buttons/list | PhenomenaAction; transport/selection identities retained |
| Inspect / physics overlays / notes | `inspect.rs`, `physics_view.rs`, `notes/panel.rs`; shared threads and Markdown links | InspectAction owner; source-linked annotations/selection unchanged; ordinary conversion precedes typing shortcut refusal |
| CAD top/name/attach | `cad/panel/name.rs`, attach.rs | captured CadAction → existing handle; cad:* ids and REST projection unchanged |
| CAD menus/palette/forms/numeric | `cad/surfaces/{mod,form}.rs`, numeric.rs, tree/{input,popup}.rs | document/session source refusal; catalogue monotonic form_sequence and op/began; guarded submission → same ops owner |
| CAD physical/material/component/composition | inspector/{entry,editors}.rs, materials/panel.rs, components/ui.rs, composition/ui.rs | existing parameter/physical source validation; material modal lifetime stamps; no geometry edit added |
| CAD files/results/references/experiments | files/form.rs, results/forms.rs, references/input.rs, experiments/ui.rs, experiment_review/ui.rs | captured source/session guards; existing command/job owners and validations |
| CAD motion/views/display | motion/ui.rs, views/panel.rs, display/{entry,ui}.rs | existing validated motion/review commands; view cube CameraAction remains shared camera |
| CAD annotation threads/pins | threads/{input,pins}.rs, kit threads | existing ThreadsAction/CadAction and provenance |
| CAD tree selectable rows | tree/rows.rs and activation::tree_keyboard | Ordinary+KeyboardOnly selection by stable TreeRowId; compound pointer owner retained |
| Place / annotation service | place_view.rs, annotations/ | no separate ordinary adapter; camera/key/photo surfaces and parent-owned kit thread controls |

CAD `activation.rs` stamps only newly rendered CAD controls, excluding Persistent
shell ancestry, even before Last's mode-scope assignment. Missing, replaced
source and form-session stamps refuse conversion. Internal serde-skipped
CadAction::Captured is checked again by handle, then delegates to the same owner;
REST projection unwraps it rather than changing public identities. File/results/
material/tree modal restoration respects ordinary_focused and cannot steal focus
from a Tab-selected button. Build's analogous RenderedUi guards both conversion
and application. These guards do not change physical definitions or undo.

## Named exceptions

- Hardware jog: `robot/hardware/actions/input.rs::jog_buttons` pairs press and
  release; rendered JogButton carries HeldControl and has no generic keyboard
  activation. Q/A releases, Z/Escape STOP, window_loss, panel/mode/window loss,
  pending-close motion refusal, shutdown STOP and supervisor safety are retained.
- Hardware sliders retain held-target/commit behavior; lesson time/narration,
  robot recorded seek and Phenomena sliders remain pinned SliderValue polling.
- Robot W/A/S/D teleoperation and planar held arrows retain paired release.
  Existing MotionRequest::Key on-screen controls are intentionally latched
  commands; planar on-screen jog is a discrete delta, not a held motor input.
- CAD tree pointer selection/release, drag, context and double-click remain one
  compound gesture owner; keyboard selection uses KeyboardOnly, without a second
  pointer selection. Robot row double-click remains a pointer gesture beside
  ordinary single selection. Radial positioning and release, viewport drags,
  face/plane/sketch picking, reference calibration and display camera gestures
  retain existing owners. No feature keyboard text loop was added.
- Builder placement/drag, library preview-plus-drag, Alt live-run grab, schematic
  pointer gestures, discussion/marker hover, lesson sketch strokes/chart hover,
  all camera/pointer surfaces remain continuous gestures or styling.

## Representative traces and written evidence

Picker actual kit entry → pinned Press/Enter/Space → Activate → original entity
Activated → picker rendered revision check → captured choice → WindowAction →
switch handle. Switcher uses the same capture with ModeButton and WindowAction.
CAD catalogue form uses FormPart → captured source/session → CadFormSet/Submit →
CadAction::Captured refusal → ops validated handler. Refinement uses Hit::Action
or stable stamped Focus → StudyAction owner → shared experiment_study refinement
commands/jobs. Close actual kit control → CloseAction → existing close lifecycle;
preference acknowledgment/preservation and hardware STOP remain independent.
System_ui discovery retains UI Button/action/Enabled/labels and invokes existing
validated action routes; it never fabricates Interaction. REST routing remains
app::actions::registry/route and shared authoritative owners.

Written fixtures: `ui_kit/activation_tests.rs` drives actual kit rendered controls
through pinned pointer/keyboard observers, checking primary press timing, repeat,
secondary refusal, hidden/disabled/despawned eligibility, Tab/Shift+Tab, shortcut
consumption, durable editor submit/draft and rebuild retention. Actual hardware
fragments at `robot/hardware/actions/input.rs` check STOP and held-jog exclusion.
`app/picker/activation_tests.rs`, switcher fixtures, Close ui_tests, CAD flow_tests
and surfaces/tests, Study ui_tests inspect actual rendered conversion, source
replacement and identities. Conversion fixtures that insert Activated directly
prove conversion/refusal only, not input dispatch. Existing owner tests updated
from synthetic Interaction remain owner fixtures. None were executed.

Checklist IDs preserved: native-keyboard-activation:task-T49.1, task-T49.2,
task-T49.3 and outcome-1 through outcome-5. Their evidence is the source contract,
family inventory, representative traces and written fixtures above; acceptance
is pending the orchestrator's independent source review. No executed or GUI
acceptance is implied.
