// Drive-input stop rules and the bindings-override fallback (web/viewer/drive-input.mjs).
// WRITTEN BUT NOT RUN: authored by reading only (syntax checked with `node --check`).
// Run with `node web/tests/drive_input.mjs`; no build or browser needed.
// `driveDeviceAxes` is a fake standing in for sim-web's drive_device_axes: it
// exercises the page-side state machine, not Rust's binding semantics.
import assert from 'node:assert/strict';
import {createDriveInput, loadDriveBindings, isTextField, isTextEntry, isChord, gamepadSnapshot, drivePeriodsPerChunk, zeroAxes, STOP, DRIVE_BINDINGS_KEY} from '../viewer/drive-input.mjs';

const bindings = {schema: 'test', keyboard: {axes: [{key: 'KeyW', axis: 'forward', direction: 1}, {key: 'KeyS', axis: 'forward', direction: -1}, {key: 'KeyA', axis: 'yaw', direction: 1}, {key: 'KeyQ', axis: 'lateral', direction: 1}],
  actions: [{key: 'KeyX', action: 'stop'}, {key: 'KeyB', action: 'halt'}]}, gamepad: {deadzone: 0.15, axes: [], buttons: []}};
const supported = [true, false, true];
const calls = [];
function fakeAxes(bindingsJson, devicesJson, supportedJson) {
  assert.equal(bindingsJson, JSON.stringify(bindings)); assert.equal(supportedJson, JSON.stringify(supported));
  const devices = JSON.parse(devicesJson), sup = JSON.parse(supportedJson); calls.push(devices);
  const kb = {forward: 0, lateral: 0, yaw: 0}, gp = {forward: 0, lateral: 0, yaw: 0}, held_actions = [];
  for (const k of devices.keys) {
    const a = bindings.keyboard.axes.find(b => b.key === k); if (a) kb[a.axis] += a.direction;
    const act = bindings.keyboard.actions.find(b => b.key === k); if (act) held_actions.push({input: `key ${k}`, action: act.action});
  }
  for (const g of devices.gamepads) {
    const y = g.axes[1] ?? 0; if (Math.abs(y) > 0.15) gp.forward += -y;
    if (g.pressed[0]) held_actions.push({input: 'button south', action: 'stop'});
  }
  const names = ['forward', 'lateral', 'yaw'], ignored = names.filter((n, i) => !sup[i] && (kb[n] !== 0 || gp[n] !== 0));
  const axes = Object.fromEntries(names.map((n, i) => [n, sup[i] ? Math.max(-1, Math.min(1, kb[n] + gp[n])) : 0]));
  const kbOn = names.some((n, i) => sup[i] && kb[n] !== 0), gpOn = names.some((n, i) => sup[i] && gp[n] !== 0);
  return JSON.stringify({axes, ignored, source: kbOn && gpOn ? 'keyboard+gamepad' : kbOn ? 'keyboard' : gpOn ? 'gamepad' : null, keyboard: kb, gamepad: gp, held_actions, ignored_pads: []});
}
const make = () => createDriveInput({driveDeviceAxes: fakeAxes, bindings, supported, resendMs: 50});
const pad = (y, south = false) => [{mapping: 'standard', id: 'pad', axes: [0, y, 0, 0], buttons: [south ? 1 : 0], pressed: [south]}];
const ahead = {axes: {forward: 1, lateral: 0, yaw: 0}};

{ // Holding sends axes at a bounded cadence; release sends exactly one zero request.
  const d = make();
  assert.deepEqual(d.keyDown({code: 'KeyW'}), []);
  assert.deepEqual(d.poll([], 0), [ahead]);
  assert.deepEqual(d.poll([], 16), [], 'unchanged axes are not re-sent within the cadence');
  assert.deepEqual(d.poll([], 60), [ahead], 'unchanged axes are re-sent after the cadence');
  d.keyDown({code: 'KeyA'});
  assert.deepEqual(d.poll([], 70), [{axes: {forward: 1, lateral: 0, yaw: 1}}], 'a change is sent at once');
  d.keyUp('KeyA'); d.keyUp('KeyW');
  assert.deepEqual(d.poll([], 80), [zeroAxes()]);
  assert.deepEqual(d.poll([], 200), [], 'one zero request only');
}
{ // Blur / hidden page / pause: "stop", and held keys stay disarmed until released.
  const d = make();
  d.keyDown({code: 'KeyW'}); assert.deepEqual(d.poll([], 0), [ahead]);
  assert.deepEqual(d.stop(), [STOP]);
  assert.deepEqual(d.poll([], 100), [], 'no axes and no extra zero after a stop');
  d.keyDown({code: 'KeyW', repeat: true});
  assert.deepEqual(d.poll([], 200), [], 'auto-repeat does not re-arm a disarmed key');
  assert.deepEqual(d.state().disarmedKeys, ['KeyW']);
  d.keyUp('KeyW'); d.keyDown({code: 'KeyW'});
  assert.deepEqual(d.poll([], 300), [ahead], 'a fresh press after release drives again');
  d.keyDown({code: 'KeyW'});
  assert.deepEqual(d.stop(), [STOP]);
  d.keyDown({code: 'KeyW', repeat: false});
  assert.deepEqual(d.poll([], 400), [ahead], 'a fresh non-repeat keydown proves a missed keyup');
}
{ // Escape always stops, except when a text field owns it.
  const d = make();
  d.keyDown({code: 'KeyW'}); d.poll([], 0);
  assert.deepEqual(d.keyDown({code: 'Escape', textTarget: true}), [], 'Escape in a text field belongs to the field');
  assert.deepEqual(d.poll([], 10), [], 'still driving, unchanged within the cadence');
  assert.deepEqual(d.keyDown({code: 'Escape'}), [STOP]);
  assert.deepEqual(d.poll([], 20), []);
  assert.deepEqual(make().keyDown({code: 'Escape', chord: true}), [STOP], 'Escape stops even in a chord');
}
{ // A bound action's rising edge: zero first (if driving), then the action, then disarm.
  const d = make();
  d.keyDown({code: 'KeyW'}); d.poll([], 0);
  d.keyDown({code: 'KeyX'});
  assert.deepEqual(d.poll([], 10), [zeroAxes(), {action: {name: 'stop'}}]);
  assert.deepEqual(d.poll([], 100), [], 'both held keys are disarmed; no repeat of the action');
  d.keyUp('KeyX'); d.keyUp('KeyW');
  assert.deepEqual(d.poll([], 110), []);
  d.keyDown({code: 'KeyB'});
  assert.deepEqual(d.poll([], 120), [{action: {name: 'halt'}}], 'no zero request when nothing was being sent');
  assert.deepEqual(d.poll([], 130), []);
}
{ // A text field taking focus disarms keys; one stop only if the keyboard was driving.
  const d = make();
  d.keyDown({code: 'KeyW'}); d.poll([], 0);
  assert.deepEqual(d.textFocus(), [STOP]);
  assert.deepEqual(d.poll([], 100), []);
  assert.deepEqual(d.textFocus(), [], 'not driving any more');
  d.keyUp('KeyW');
  assert.deepEqual(d.keyDown({code: 'KeyW', textTarget: true}), []);
  assert.deepEqual(d.poll([], 200), [], 'keys typed into a field do not drive');
  const g = make();
  g.poll(pad(0), 0); assert.deepEqual(g.poll(pad(-1), 10), [ahead], 'stick up drives forward');
  assert.deepEqual(g.textFocus(), [], 'a gamepad-only drive is not stopped by text focus');
}
{ // Cmd/Ctrl/Alt chords are not driving keys, and a chord disarms keys already held.
  const d = make();
  d.keyDown({code: 'KeyW', chord: true});
  assert.deepEqual(d.poll([], 0), []);
  d.keyUp('KeyW'); d.keyDown({code: 'KeyW'}); assert.deepEqual(d.poll([], 10), [ahead]);
  d.keyDown({code: 'MetaLeft', chord: true});
  assert.deepEqual(d.poll([], 20), [zeroAxes()], 'release rule: the chord disarmed W');
  // A chord press, modifier released, then auto-repeats of the same key: never drives.
  const r = make();
  r.keyDown({code: 'KeyW', chord: true}); r.keyUp('MetaLeft');
  r.keyDown({code: 'KeyW', repeat: true}); r.keyDown({code: 'KeyW', repeat: true});
  assert.deepEqual(r.poll([], 0), [], 'repeats of a chorded press stay disarmed');
  r.keyUp('KeyW'); r.keyDown({code: 'KeyW'}); assert.deepEqual(r.poll([], 10), [ahead], 'its keyup cleared it');
  // A repeat of a key never seen pressed (held before focus or load) is held and disarmed.
  const u = make();
  u.keyDown({code: 'KeyW', repeat: true}); assert.deepEqual(u.poll([], 0), []);
  assert.deepEqual(u.state().disarmedKeys, ['KeyW']);
  // A press typed into a text field, then repeats after focus leaves: never drives.
  const t = make();
  t.keyDown({code: 'KeyW', textTarget: true, textEntry: true}); t.keyDown({code: 'KeyW', repeat: true});
  assert.deepEqual(t.poll([], 0), []);
  assert.equal(isChord({metaKey: true}), true); assert.equal(isChord({ctrlKey: false, altKey: false, metaKey: false}), false);
}
{ // Unsupported axes are never sent (Rust zeroes them; the page sends what Rust answers).
  const d = make();
  d.keyDown({code: 'KeyQ'});
  assert.deepEqual(d.poll([], 0), []);
  assert.deepEqual(d.state().last.ignored, ['lateral']);
}
{ // A disarmed pad is ignored until its sticks are neutral and its buttons released.
  const d = make();
  assert.deepEqual(d.poll(pad(-1), 0), [], 'a pad deflected at load starts disarmed');
  assert.equal(d.state().padDisarmed, true);
  assert.deepEqual(d.poll(pad(-0.1), 10), [], 'inside the deadzone re-arms');
  assert.equal(d.state().padDisarmed, false);
  assert.deepEqual(d.poll(pad(-1), 20), [ahead]);
  assert.deepEqual(d.stop(), [STOP]);
  assert.deepEqual(d.poll(pad(-1), 100), [], 'still deflected: ignored');
  d.keyDown({code: 'KeyA'});
  assert.deepEqual(d.poll(pad(-1, true), 110), [{axes: {forward: 0, lateral: 0, yaw: 1}}], 'the keyboard drives while the pad is disarmed; its button is not an action');
  d.keyUp('KeyA');
  assert.deepEqual(d.poll(pad(0, true), 120), [zeroAxes()], 'button still held: pad stays disarmed');
  assert.equal(d.state().padDisarmed, true);
  assert.deepEqual(d.poll(pad(0), 130), []);
  assert.equal(d.state().padDisarmed, false);
  assert.deepEqual(d.poll(pad(0, true), 140), [{action: {name: 'stop'}}], 'an armed button press sends its action');
  assert.equal(d.state().padDisarmed, true, 'and disarms the pad');
  assert.deepEqual(d.poll(pad(0, true), 150), []);
}
{ // Panel action buttons go through the state machine: zero first, then the action, then disarm.
  const d = make();
  d.poll(pad(0), 0); assert.deepEqual(d.poll(pad(-1), 10), [ahead]);
  assert.deepEqual(d.action('halt'), [zeroAxes(), {action: {name: 'halt'}}]);
  assert.deepEqual(d.poll(pad(-1), 20), [], 'the held stick does not re-send axes after the action');
  assert.deepEqual(d.poll(pad(-1), 100), [], 'still disarmed after the resend cadence');
  assert.deepEqual(d.action('halt'), [{action: {name: 'halt'}}], 'no zero when nothing was being sent');
}
{ // Escape stops when a select, checkbox or slider has focus; only text entry keeps it.
  const d = make();
  assert.deepEqual(d.keyDown({code: 'Escape', textTarget: true, textEntry: false}), [STOP]);
  assert.deepEqual(d.keyDown({code: 'Escape', textTarget: true, textEntry: true}), []);
  assert.equal(isTextEntry({tagName: 'INPUT', type: 'text'}), true); assert.equal(isTextEntry({tagName: 'INPUT', type: 'search'}), true);
  assert.equal(isTextEntry({tagName: 'INPUT'}), true); assert.equal(isTextEntry({tagName: 'TEXTAREA'}), true);
  assert.equal(isTextEntry({tagName: 'DIV', isContentEditable: true}), true);
  assert.equal(isTextEntry({tagName: 'SELECT'}), false); assert.equal(isTextEntry({tagName: 'INPUT', type: 'checkbox'}), false);
  assert.equal(isTextEntry({tagName: 'INPUT', type: 'range'}), false); assert.equal(isTextEntry(null), false);
}
console.log('drive input stop rules passed');

{ // Bindings: defaults, accepted override, refused override falls back visibly.
  const defaults = JSON.stringify({schema: 'sim.drive-bindings/1', stored: false, bindings: {schema: 'sim.drive-bindings/1', keyboard: {axes: [], actions: []}, gamepad: {deadzone: 0.15, axes: [], buttons: []}}, describe: [{input: 'W', does: 'forward'}]});
  const validate = text => { const v = JSON.parse(text); if (v.keyboard?.axes?.[2]?.key === 'Space') throw 'drive_bindings.keyboard.axes[2].key: `Space` is the UI kit\'s activation key'; return JSON.stringify({...JSON.parse(defaults), stored: true, bindings: v}); };
  const storage = value => ({getItem: key => { assert.equal(key, DRIVE_BINDINGS_KEY); return value; }});
  const none = loadDriveBindings({defaultBindings: () => defaults, validateBindings: validate, storage: storage(null)});
  assert.equal(none.stored, false); assert.equal(none.refusal, null); assert.deepEqual(none.describe, [{input: 'W', does: 'forward'}]);
  const good = {schema: 'sim.drive-bindings/1', keyboard: {axes: [{key: 'KeyI', axis: 'forward', direction: 1}]}, gamepad: {deadzone: 0.2}};
  const accepted = loadDriveBindings({defaultBindings: () => defaults, validateBindings: validate, storage: storage(JSON.stringify(good))});
  assert.equal(accepted.stored, true); assert.equal(accepted.refusal, null); assert.deepEqual(accepted.bindings, good);
  const bad = {...good, keyboard: {axes: [{}, {}, {key: 'Space'}]}};
  const refused = loadDriveBindings({defaultBindings: () => defaults, validateBindings: validate, storage: storage(JSON.stringify(bad))});
  assert.equal(refused.stored, false, 'refusal falls back to the defaults');
  assert.deepEqual(refused.bindings, JSON.parse(defaults).bindings);
  assert.match(refused.refusal, /drive_bindings\.keyboard\.axes\[2\]\.key/, 'the reason names the field');
  const unreadable = loadDriveBindings({defaultBindings: () => defaults, validateBindings: validate, storage: {getItem() { throw new Error('SecurityError'); }}});
  assert.equal(unreadable.stored, false); assert.match(unreadable.refusal, /SecurityError/);
}
console.log('drive bindings fallback passed');

assert.equal(isTextField({tagName: 'INPUT'}), true); assert.equal(isTextField({tagName: 'TEXTAREA'}), true);
assert.equal(isTextField({tagName: 'SELECT'}), true); assert.equal(isTextField({tagName: 'DIV', isContentEditable: true}), true);
assert.equal(isTextField({tagName: 'BUTTON'}), false); assert.equal(isTextField(null), false);
assert.deepEqual(gamepadSnapshot([null, {mapping: 'standard', id: 'x', connected: true, axes: [0.5, -1], buttons: [{value: 1, pressed: true}, {value: 0.25, pressed: false}]}]),
  [{mapping: 'standard', id: 'x', axes: [0.5, -1], buttons: [1, 0.25], pressed: [true, false]}]);
assert.deepEqual(gamepadSnapshot([{mapping: 'standard', id: 'y', axes: [NaN, Infinity, 0.5], buttons: [{value: NaN, pressed: false}]}]),
  [{mapping: 'standard', id: 'y', axes: [0, 0, 0.5], buttons: [0], pressed: [false]}], 'non-finite readings are sent as 0');
assert.equal(drivePeriodsPerChunk(0.02), 3); assert.equal(drivePeriodsPerChunk(1), 1); assert.equal(drivePeriodsPerChunk(1e-6), 1000);
assert.throws(() => drivePeriodsPerChunk(0)); assert.throws(() => drivePeriodsPerChunk(NaN));
console.log('drive input helpers passed');
