# Linked assembly and schematic — 2026-09-20

Run `./examples/systems-viewer/run-linked.sh` from the repository root. This
opens compact native Bevy and egui windows over the same sealed description.
The launch script uses two low-priority build jobs; the GUIs run at normal
priority. Source/geometry hashes are retained in `input-hashes.json`.

## Observed interactions on the Intel Mac

- Picked the blue supply mesh: the assembly became gold and the schematic
  selected `example/motor-thermal/supply`, showing its 2 V source parameter.
- Picked the schematic rotor card: the assembly shaft/flywheel became gold and
  both inspectors showed the rotor inertia/damping.
- Picked `case.node` in the schematic: the assembly highlighted the thermal
  storage and its real incident thermal connection, with exact port identity.
- Picked the three-terminal thermal wire in the schematic: assembly highlighted
  motor, case and cooling together; both views retained all three terminals.
- Picked the assembly's electrical connection hub: schematic highlighted motor
  and supply, with exactly `motor.plug.electrical` and `supply.p`. The retained
  `electrical-selection.json` is the actual session record, not a manufactured
  expected result. Screenshots capture only these application windows.
- Closed assembly left the schematic alive. Reattached assembly to the same
  session; the shared selection was restored without reopening the schematic.

The component, port and net updates were visible in the peer window after each
click. No end-to-end latency percentile or large-scene performance claim is
made. User camera/presentation exploration also remained possible between checks.

## Automated verification

46 tests passed: 33 in `sim-inspect` / `sim-diagram`, 11 in `sim-viewer`, and 2
in `sim-spatial`. The adjacent logs retain individual results. Native builds
passed and the graphics-free `sim-inspect` contract checked on
`wasm32-unknown-unknown`. Existing runtime dead-code and third-party `block`
future-compatibility warnings remain; no check failed.

Coverage includes full source membership for collapsed bundles, exact-net
inspection/annotation scope, no echo, newer local input, peer closure and
reconnection, duplicate-role and foreign-state rejection, hidden selected
geometry, and preservation of physical inputs, unsaved notes, pins and camera.
The ordinary workspace sidecar/undo/restore and schematic interaction cases also
passed. Browser rendering, Windows transport and large-scene performance were
not tested in this increment.

`electrical-selection-*` captures the bidirectional GUI check before final text
polish. `reconnected-*` captures the subsequent assembly reattachment and the
final assembly labels/wrapping. The collapsed-bundle inspector edge case was
verified automatically; the native GUI fixture has no collapsed bundles. No
further desktop automation was performed after the user requested a pause while
watching shows. Physical input hashes were rechecked afterward and are unchanged.

## Scope

The geometry remains illustrative. No live samples, motion simulation, LED,
CAD meshes, heat fields or fluid field solver were added. Diagram projection,
notes and layout remain presentation state. Linked selection never changes the
sealed model, CAD data, registry parameters or accepted runtime observations.
The temporary transport is Unix-native; browser spatial/link acceptance is a
later checkpoint. This is a two-window GUI increment for feedback.
