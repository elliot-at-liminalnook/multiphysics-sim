// Truthful display for drive-profile presets: every number shown comes from
// Rust (DriveSimulation status/metadata, drive_device_axes, the built drive),
// with the units Rust declares. Labels refresh at a steady rate.
const AXES = ['forward', 'lateral', 'yaw'];
const TWIST_KEYS = ['forward_m_s', 'lateral_m_s', 'yaw_rad_s'];
const REFRESH_MS = 250;

function el(tag, text, attrs = {}) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  return node;
}
const fixed = (v, digits = 3) => Number.isFinite(v) ? (Object.is(Number(v.toFixed(digits)), -0) ? 0 : v).toFixed(digits) : '—';
const short = hash => typeof hash === 'string' ? hash.slice(0, 12) : 'none';
const pick = (value, i) => Array.isArray(value) ? value[i] : value;

/**
 * Build the panel in `container`. `drive` is the built EmbeddedDrive (parsed),
 * `metadata` DriveSimulation.metadata(), `bindings` the loadDriveBindings
 * answer. `onAction(name)` asks the input state machine to send the profile action
 * `name` (it sends a zero request first if axes were being sent, then
 * disarms held inputs).
 */
export function createDrivePanel(container, {drive, metadata, bindings, onAction}) {
  container.replaceChildren(); container.hidden = false;
  const units = metadata.limits?.speed_units ?? [];
  const accelUnits = metadata.limits?.accel_units ?? [];
  const scope = el('div', 'Browser compatibility path · unexecuted. The binding\'s embedded Rhai adapter runs in Rust/WASM in this page\'s worker; the native viewer runs the bound external controller. Requests only: Rust applies the profile\'s limits, mixing and deadman on simulation time.', {class: 'notice', 'data-state': 'experimental'});
  const fidelity = el('p'); fidelity.append(el('strong', 'Fidelity: '), document.createTextNode(String(drive.fidelity ?? metadata.fidelity ?? 'not stated')));
  const id = drive.identity ?? metadata.identity ?? {};
  const identity = el('p', `Binding ${id.binding} · profile ${id.profile} · entry ${id.entry}. sha256: script ${short(id.script_sha256)} · config ${short(id.config_sha256)} · profile ${short(id.profile_sha256)} · model ${short(id.model_sha256)} · CAD ${short(id.cad_sha256)}`, {class: 'muted'});
  identity.title = JSON.stringify(id, null, 1);

  const actions = el('div', undefined, {class: 'drive-actions'});
  for (const action of metadata.actions ?? []) {
    const button = el('button', action.name); button.title = action.description ?? '';
    button.dataset.driveAction = action.name;
    button.onclick = () => onAction(action.name);
    actions.append(button);
  }
  actions.append(el('p', 'Esc always requests stop (except inside a text field). Leaving the page or window also requests stop.', {class: 'muted'}));

  const live = el('dl', undefined, {class: 'drive-live'});
  const rows = {};
  for (const [key, label] of [['device', 'Driving'], ['requested', 'Requested twist'], ['commanded', 'Commanded twist'], ['deadman', 'Deadman'], ['heartbeat', 'Heartbeat'], ['ignored', 'Ignored axes'], ['pads', 'Gamepads']]) {
    live.append(el('dt', label)); rows[key] = el('dd', '—'); live.append(rows[key]);
  }

  const limits = el('table', undefined, {class: 'drive-table'});
  const head = el('tr'); for (const h of ['Axis', 'Max speed', 'Max accel', 'Stop decel']) head.append(el('th', h)); limits.append(head);
  AXES.forEach((axis, i) => {
    const row = el('tr'), supported = metadata.limits?.supported?.[i];
    row.append(el('td', supported ? axis : `${axis} (not supported)`));
    const stop = pick(metadata.limits?.stop_decel, i);
    row.append(el('td', supported ? `${fixed(metadata.limits.max_speed?.[i])} ${units[i] ?? ''}` : '—'));
    row.append(el('td', supported ? `${fixed(metadata.limits.max_accel?.[i])} ${accelUnits[i] ?? ''}` : '—'));
    row.append(el('td', supported && stop != null ? `${fixed(stop)} ${accelUnits[i] ?? ''}` : '—'));
    limits.append(row);
  });
  const deadman = metadata.deadman ?? {};
  const deadmanText = el('p', `Deadman: ${fixed(deadman.timeout_s)} s of simulation time without a fresh request, then ${deadman.on_loss ?? '—'}.`, {class: 'muted'});

  const bindingSource = el('p', bindings.stored ? 'Bindings: stored override (localStorage sim.drive-bindings/1), accepted by Rust.' : 'Bindings: defaults from Rust (no accepted stored override).', {class: 'muted'});
  const refusal = el('div', bindings.refusal ?? '', {class: 'notice', 'data-state': 'failed'}); refusal.hidden = !bindings.refusal;
  const describe = el('table', undefined, {class: 'drive-table'});
  const dh = el('tr'); dh.append(el('th', 'Input'), el('th', 'Does')); describe.append(dh);
  for (const d of bindings.describe ?? []) { const r = el('tr'); r.append(el('td', d.input), el('td', d.does)); describe.append(r); }

  container.append(scope, fidelity, identity, el('h3', 'Drive'), actions, live, el('h3', 'Profile limits'), limits, deadmanText, el('h3', 'Bindings'), bindingSource, refusal, describe);

  let status = null, input = null, dirty = true, shownAt = -Infinity;
  const twist = t => t ? AXES.map((axis, i) => `${axis} ${fixed(t[TWIST_KEYS[i]])} ${units[i] ?? ''}`).join(' · ') : '—';
  function render(now) {
    if (!dirty || now - shownAt < REFRESH_MS) return;
    dirty = false; shownAt = now;
    const last = input?.last;
    rows.device.textContent = last?.source ?? 'none (neutral)';
    rows.requested.textContent = twist(status?.request);
    rows.commanded.textContent = twist(status?.commanded);
    rows.deadman.textContent = status ? `age ${fixed(status.age_s, 2)} / ${fixed(deadman.timeout_s, 2)} s · ${status.expired ? 'EXPIRED (stopping)' : 'fresh'}${status.halted ? ' · halted' : ''}` : '—';
    rows.heartbeat.textContent = status ? `${status.heartbeat} · sim ${fixed(status.time_s, 2)} s · ${status.periods} periods` : '—';
    rows.ignored.textContent = last?.ignored?.length ? `${last.ignored.join(', ')} (not in this profile; not sent)` : 'none';
    const pads = [...(input?.ignoredPads ?? [])];
    if (input?.padDisarmed) pads.unshift('disarmed until sticks are neutral and buttons released');
    if (input?.disarmedKeys?.length) pads.push(`keys disarmed until released: ${input.disarmedKeys.join(', ')}`);
    rows.pads.textContent = pads.length ? pads.join(' · ') : 'armed';
  }
  return {
    status(next) { if (next) { status = next; dirty = true; } },
    input(next) { input = next; dirty = true; },
    render,
  };
}
