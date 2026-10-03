// Device reading for drive-profile presets (browser compatibility path).
//
// This module only tracks which physical inputs are held, which are disarmed
// by a stop, and when to send a request. Rust decides everything else through
// the injected `driveDeviceAxes` (sim-web's `drive_device_axes`): bindings,
// deadzone, stick inversion, the W3C stick-Y sign, unsupported axes and the
// sum of devices; and, in the worker's DriveSimulation, the profile's scaling,
// limits, acceleration, mixing and the deadman on simulation time. No numbers
// are scaled, clamped or mixed here: axes go out exactly as Rust answered them.

/** localStorage key of an optional bindings override (`sim.drive-bindings/1`). */
export const DRIVE_BINDINGS_KEY = 'sim.drive-bindings/1';
/** While nonzero axes are held, an unchanged request is re-sent this often (wall ms). */
export const AXES_RESEND_MS = 50;
/** Simulated time each live work chunk targets (s); a cadence choice, not physics. */
export const DRIVE_CHUNK_S = 0.05;
/** The profile's stop request (Rust `DriveRequest::Stop`). */
export const STOP = 'stop';

/** A zero axes request (fresh object each call). */
export function zeroAxes() { return {axes: {forward: 0, lateral: 0, yaw: 0}}; }

/** The message of a value thrown by wasm (a string) or JS (an Error). */
export function errorText(error) {
  return typeof error === 'string' ? error : error?.message ?? String(error);
}

/**
 * The bindings answer to use: the stored override when Rust accepts it, else
 * the defaults with `refusal` naming why (shown to the user verbatim).
 * `defaultBindings()` / `validateBindings(text)` are sim-web's
 * `default_drive_bindings` / `validate_drive_bindings` (JSON text in and out;
 * validation throws a string naming the field).
 */
export function loadDriveBindings({defaultBindings, validateBindings, storage}) {
  let text = null;
  try { text = storage ? storage.getItem(DRIVE_BINDINGS_KEY) : null; }
  catch (error) {
    return {...JSON.parse(defaultBindings()), refusal: `localStorage ${DRIVE_BINDINGS_KEY} could not be read (${errorText(error)}); using the default bindings`};
  }
  if (text === null) return {...JSON.parse(defaultBindings()), refusal: null};
  try { return {...JSON.parse(validateBindings(text)), refusal: null}; }
  catch (error) {
    return {...JSON.parse(defaultBindings()), refusal: `Stored bindings (localStorage ${DRIVE_BINDINGS_KEY}) refused: ${errorText(error)}. Using the default bindings.`};
  }
}

/** Elements that own the keyboard while focused: input, textarea, select, contenteditable. */
export function isTextField(element) {
  if (!element) return false;
  if (element.isContentEditable) return true;
  return element.tagName === 'INPUT' || element.tagName === 'TEXTAREA' || element.tagName === 'SELECT';
}

// <input> types that do not take typed text (Escape still stops while they have focus).
const NON_TEXT_INPUTS = new Set(['checkbox', 'radio', 'range', 'button', 'submit', 'reset', 'file', 'color', 'image', 'hidden']);
/** Elements where Escape belongs to the field: text-entry inputs, textarea, contenteditable (not select, checkbox or slider). */
export function isTextEntry(element) {
  if (!element) return false;
  if (element.isContentEditable) return true;
  if (element.tagName === 'TEXTAREA') return true;
  return element.tagName === 'INPUT' && !NON_TEXT_INPUTS.has(String(element.type || 'text').toLowerCase());
}

/** A Cmd/Ctrl/Alt chord is never a driving key. */
export function isChord(event) { return Boolean(event.metaKey || event.ctrlKey || event.altKey); }

// A non-finite reading (a disconnected or misbehaving pad) is sent as 0, never NaN.
const finite = v => { const n = Number(v); return Number.isFinite(n) ? n : 0; };
/** `navigator.getGamepads()` as the plain values Rust reads (`W3cGamepad`). */
export function gamepadSnapshot(list) {
  return [...(list ?? [])].filter(g => g && g.connected !== false).map(g => ({
    mapping: String(g.mapping ?? ''), id: String(g.id ?? ''),
    axes: [...g.axes].map(finite),
    buttons: [...g.buttons].map(b => finite(b?.value)),
    pressed: [...g.buttons].map(b => Boolean(b?.pressed)),
  }));
}

/** Control periods per live work chunk (bounded 1..1000 as the worker requires). */
export function drivePeriodsPerChunk(periodS) {
  if (!Number.isFinite(periodS) || periodS <= 0) throw new Error(`drive metadata period_s must be a positive number, got ${periodS}`);
  return Math.max(1, Math.min(1000, Math.round(DRIVE_CHUNK_S / periodS)));
}

const isZero = a => a.forward === 0 && a.lateral === 0 && a.yaw === 0;

/**
 * The input state machine. `driveDeviceAxes(bindingsJson, devicesJson,
 * supportedJson) -> string` is Rust's `drive_device_axes`; `bindings` is the
 * bindings file (the answer's `bindings`), `supported` the profile's
 * supported axes (metadata `limits.supported`). Every method returns the
 * requests to send, in order (DriveRequest JSON values).
 *
 * Rules:
 * - while axes are nonzero, `{axes}` is sent when they change and at least
 *   every `resendMs`; when every input is neutral again, one zero request;
 * - a bound action's rising edge sends a zero request first (if axes were
 *   being sent), then the action, then disarms every held input;
 * - `action(name)` (the panel's action buttons) does the same as a bound
 *   action's edge: a zero request if axes were being sent, the action, then
 *   disarms every held input;
 * - `stop()` (blur, hidden page, Escape, pause, the panel's Stop) sends
 *   "stop" and disarms every held input;
 * - a disarmed key is left out of `keys` until its keyup (or a fresh,
 *   non-repeat keydown, which proves a missed keyup); a disarmed pad is
 *   ignored until Rust reports zero gamepad axes and no held button action;
 * - a text field (input, textarea, select, contenteditable) taking focus
 *   disarms the keys and, if the keyboard was driving, sends one stop;
 *   keydowns in a text field and Cmd/Ctrl/Alt chords are not driving keys:
 *   the key is recorded as held and disarmed, so its auto-repeats never drive
 *   and its keyup clears it (a chord also disarms keys already held, since a
 *   platform may not deliver their keyup while the modifier is down); an
 *   auto-repeat of a key whose press was never seen (held before focus or
 *   load) is likewise held and disarmed;
 * - Escape always stops, except when a text-entry element (`textEntry`:
 *   text-like input, textarea, contenteditable) has focus.
 */
export function createDriveInput({driveDeviceAxes, bindings, supported, resendMs = AXES_RESEND_MS}) {
  const bindingsJson = JSON.stringify(bindings), supportedJson = JSON.stringify(supported);
  const bound = new Set([...(bindings?.keyboard?.axes ?? []).map(a => a.key), ...(bindings?.keyboard?.actions ?? []).map(a => a.key)]);
  const held = new Set(), disarmed = new Set();
  // A pad starts disarmed: a stick already deflected at load does not drive until it is neutral.
  let padDisarmed = true, sendingAxes = false, sentAxesJson = null, sentAt = -Infinity, sentSource = null;
  let previousActions = new Set(), last = null, ignoredPads = [];
  const evaluate = devices => JSON.parse(driveDeviceAxes(bindingsJson, JSON.stringify(devices), supportedJson));
  const keys = () => [...held].filter(code => !disarmed.has(code));
  function disarmAll() {
    for (const code of held) disarmed.add(code);
    padDisarmed = true;
  }
  function forgetAxes() { sendingAxes = false; sentAxesJson = null; sentSource = null; }
  const api = {
    /** Whether `code` is a bound key (the page then prevents its default action). */
    isBound(code) { return bound.has(code); },
    keyDown({code, repeat = false, chord = false, textTarget = false, textEntry = false}) {
      if (code === 'Escape') return textEntry ? [] : api.stop();
      if (chord) for (const c of held) disarmed.add(c);
      if (textTarget || chord || (repeat && !held.has(code))) { held.add(code); disarmed.add(code); return []; }
      if (!repeat) disarmed.delete(code);
      held.add(code);
      return [];
    },
    keyUp(code) { held.delete(code); disarmed.delete(code); return []; },
    stop() { disarmAll(); forgetAxes(); return [STOP]; },
    action(name) {
      const requests = sendingAxes ? [zeroAxes(), {action: {name}}] : [{action: {name}}];
      disarmAll(); forgetAxes();
      return requests;
    },
    textFocus() {
      const keyboardDriving = sendingAxes && Boolean(sentSource?.includes('keyboard'));
      for (const code of held) disarmed.add(code);
      if (!keyboardDriving) return [];
      forgetAxes();
      return [STOP];
    },
    /** One animation frame: `gamepads` from `gamepadSnapshot`, `now` in wall ms. */
    poll(gamepads, now) {
      const devicesKeys = keys();
      let result = evaluate({keys: devicesKeys, gamepads});
      ignoredPads = result.ignored_pads ?? [];
      if (padDisarmed) {
        const neutral = isZero(result.gamepad) && !result.held_actions.some(h => h.input.startsWith('button '));
        if (neutral) padDisarmed = false;
        else result = evaluate({keys: devicesKeys, gamepads: []});
      }
      last = result;
      const requests = [];
      const rising = result.held_actions.filter(h => !previousActions.has(h.input));
      previousActions = new Set(result.held_actions.map(h => h.input));
      if (rising.length) {
        if (sendingAxes) requests.push(zeroAxes());
        for (const name of new Set(rising.map(h => h.action))) requests.push({action: {name}});
        disarmAll(); forgetAxes(); previousActions = new Set();
        return requests;
      }
      const a = result.axes;
      if (!isZero(a)) {
        const json = JSON.stringify([a.forward, a.lateral, a.yaw]);
        if (!sendingAxes || json !== sentAxesJson || now - sentAt >= resendMs) {
          requests.push({axes: {forward: a.forward, lateral: a.lateral, yaw: a.yaw}});
          sendingAxes = true; sentAxesJson = json; sentAt = now; sentSource = result.source;
        }
      } else if (sendingAxes) {
        requests.push(zeroAxes());
        forgetAxes();
      }
      return requests;
    },
    /** What the panel shows: the last Rust answer and the disarm state. */
    state() {
      return {last, ignoredPads, padDisarmed, disarmedKeys: [...disarmed].filter(c => held.has(c)), heldKeys: [...held], sending: sendingAxes};
    },
  };
  return api;
}
