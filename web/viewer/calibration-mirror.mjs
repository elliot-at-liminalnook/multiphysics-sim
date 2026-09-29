// Suspended simulated robot that mirrors the physical leg's measured encoders.
// Geometry only: the shared Rust closure solver runs in its own worker and
// poses the CAD robot with its body held still. Nothing here commands motors.
const COUNTS = 4096, LIFT_M = 0.25;
const JOINTS = {'Hip servo output': 'Hip swing (belt)', 'Worm servo output': 'Worm drive', 'Foot servo output': 'Foot slide'};
// Operator-supplied role names → CAD servo joints; editable in the panel.
const DEFAULT_JOINT = {knee: 'Foot servo output', worm: 'Worm servo output', 'belt/hip': 'Hip servo output'};
const STORE = 'calibration-mirror-v1';
// Pose each motor is aligned at: CAD home, or halfway between its CAD joint
// limits. The printed knee cannot extend to its CAD home (fully down), so it
// aligns at mid-travel. The chosen angle is saved with the encoder reference.
const ALIGN = {home: 'CAD home', mid: 'Mid-travel'};
const defaultAlign = joint => joint === 'Foot servo output' ? 'mid' : 'home';

export class LegMirror {
  constructor(details, roles, visible = () => true) {
    this.visible = visible; this.roles = roles; this.coordinates = null; this.gait = null; this.gaitRealLeg = true; this.gaitInfo = null; this.busy = false; this.pending = null; this.error = null; this.last = null;
    let saved = {}; try { saved = JSON.parse(localStorage.getItem(STORE)) || {}; } catch {}
    this.settings = {enabled: saved.enabled ?? true, leg: saved.leg ?? '+X', bindings: {}};
    for (const id of Object.keys(roles)) {
      const joint = saved.bindings?.[id]?.joint ?? DEFAULT_JOINT[roles[id]] ?? 'Hip servo output';
      this.settings.bindings[id] = {joint, polarity: saved.bindings?.[id]?.polarity ?? 1, align: saved.bindings?.[id]?.align ?? defaultAlign(joint)};
    }
    details.innerHTML = `<summary>Simulated leg mirror</summary>
     <label><input type="checkbox" id="mirror-on"> Show the real leg on the suspended simulated robot</label>
     <label>Simulated leg <select id="mirror-leg">${['+X', '-X', '+Y', '-Y'].map(l => `<option>${l}</option>`).join('')}</select></label>
     <div id="mirror-rows"></div>
     <p class="small">Align each motor once: move the real leg until it matches the simulated leg's alignment pose (CAD home, or mid-travel where the real part cannot reach home), then press <b>Save sim alignment</b>. The pose's joint angle is saved with the alignment. If the simulated part turns the wrong way, flip its sign. The mirrored leg is tinted blue. Geometry only: no simulated forces, contact or motor model.</p>
     <p id="mirror-status" class="small" role="status"></p>`;
    this.el = id => details.querySelector('#mirror-' + id);
    this.el('on').checked = this.settings.enabled; this.el('leg').value = this.settings.leg;
    this.el('on').onchange = () => { this.settings.enabled = this.el('on').checked; this.save(); this.settings.enabled ? this.begin() : window.robotViewer?.end(); };
    this.el('leg').onchange = () => { this.settings.leg = this.el('leg').value; this.save(); this.begin(); };
    this.el('rows').replaceChildren(...Object.entries(roles).map(([id, role]) => {
      const row = document.createElement('div'); row.className = 'row'; row.dataset.id = id;
      row.innerHTML = `<span style="flex:.8">${role} <span class="small">ID ${id}</span></span><select class="mirror-joint" aria-label="${role} CAD joint">${Object.entries(JOINTS).map(([j, label]) => `<option value="${j}">${label}</option>`).join('')}</select><select class="mirror-sign" aria-label="${role} direction" style="flex:.4"><option value="1">+</option><option value="-1">−</option></select><select class="mirror-align" aria-label="${role} alignment pose" style="flex:.7">${Object.entries(ALIGN).map(([k, label]) => `<option value="${k}">${label}</option>`).join('')}</select>`;
      row.querySelector('.mirror-joint').value = this.settings.bindings[id].joint; row.querySelector('.mirror-sign').value = String(this.settings.bindings[id].polarity); row.querySelector('.mirror-align').value = this.settings.bindings[id].align;
      row.onchange = () => { this.settings.bindings[id] = {joint: row.querySelector('.mirror-joint').value, polarity: Number(row.querySelector('.mirror-sign').value), align: row.querySelector('.mirror-align').value}; this.save(); this.begin(); };
      return row;
    }));
    this.worker = new Worker('./worker.js', {type: 'module'}); this.serial = 0; this.requests = new Map();
    this.worker.onmessage = ({data}) => { const r = this.requests.get(data.id); if (!r) return; this.requests.delete(data.id); data.error ? r.reject(Error(data.error)) : r.resolve(data.result); };
    this.worker.onerror = e => { this.error = e.message || 'Mirror worker failed'; this.status(); };
  }
  request(type, data) { return new Promise((resolve, reject) => { const id = ++this.serial; this.requests.set(id, {resolve, reject}); this.worker.postMessage({id, type, ...data}); }); }
  save() { try { localStorage.setItem(STORE, JSON.stringify(this.settings)); } catch {} }
  status(text) { this.el('status').textContent = this.error ? 'Mirror unavailable: ' + this.error : text ?? ''; }
  /// Record of the display binding for exports; it is not a CAD joint calibration.
  record() { return {...this.settings, lift_m: LIFT_M, counts_per_revolution: COUNTS, note: 'Display-only encoder to CAD-joint binding; not promoted to CAD.'}; }
  joint(id) { return `${this.settings.leg} | ${this.settings.bindings[id].joint}`; }
  /// Joint angle to align motor `id` at now (sent with Save sim alignment);
  /// undefined until the robot model is loaded.
  alignmentAngle(id) {
    const c = this.coordinates?.find(x => x.joint === this.joint(id)); if (!c) return undefined;
    return this.settings.bindings[id].align === 'mid' && c.lower != null && c.upper != null ? (c.lower + c.upper) / 2 : c.home;
  }
  /// Joint angle a saved alignment was captured at (older saves: CAD home).
  static savedAngle(axis, c) { return axis.reference_joint_rad ?? c.home; }
  async begin() {
    if (!this.settings.enabled || !window.robotViewer || !this.visible()) return;
    const scene = window.robotViewer.scene();
    if (!scene) { this.status('Waiting for the robot model to load…'); setTimeout(() => this.begin(), 500); return; }
    try {
      if (!this.coordinates) { this.status('Preparing the suspended robot…'); this.coordinates = (await this.request('mirror_load', {scene, lift_m: LIFT_M})).coordinates; }
      const joints = new Set(Object.keys(this.settings.bindings).map(id => this.joint(id)));
      if (joints.size !== Object.keys(this.settings.bindings).length) throw Error('Bind each motor to a different CAD joint');
      for (const j of joints) if (!this.coordinates.some(c => c.joint === j)) throw Error(`CAD model has no motor joint ${j}`);
      this.error = null;
      if (!this.settings.enabled || !this.visible()) return;
      const leg = this.settings.leg + ' |', links = window.robotViewer.scene().robot.links.map(l => l.name).filter(n => n.startsWith(leg));
      window.robotViewer.begin(links); if (this.last) this.update(this.last.state, true); else this.update({}, true);
      window.robotViewer.fit();
    } catch (e) { this.error = e.message; this.status(); }
  }
  /// Called on every panel render with the server state.
  update(state, force = false) {
    this.last = {state};
    if (!this.settings.enabled || !this.coordinates || this.error) return;
    // A playing gait poses every motor joint; the bound leg shows the real
    // encoders instead when the real leg is part of the session.
    const values = this.coordinates.map(c => this.gait?.[c.joint] ?? c.home), lines = [];
    if (this.gait && !this.gaitRealLeg) { this.text = 'Simulated gait'; const key = values.join(','); if (force || key !== this.pending) { this.pending = key; this.solve(values); } return; }
    for (const [id, role] of Object.entries(this.roles)) {
      const axis = state.calibration?.axes?.[id] ?? {}, t = state.samples?.[id], raw = t?.position_continuous ?? t?.position_raw;
      const i = this.coordinates.findIndex(c => c.joint === this.joint(id)); if (i < 0) continue;
      const pose = ALIGN[this.settings.bindings[id].align].toLowerCase();
      if (axis.reference == null) { values[i] = this.alignmentAngle(id); lines.push(`${role}: not aligned — shown at ${pose}`); continue; }
      // A multi-turn alignment only holds in the encoder tracking session it was saved in.
      const multiTurn = axis.reference < 0 || axis.reference > COUNTS - 1;
      if (multiTurn && axis.reference_session !== state.coordinate_session) { values[i] = this.alignmentAngle(id); lines.push(`${role}: alignment is from an earlier session — re-align (shown at ${pose})`); continue; }
      if (raw == null) { lines.push(`${role}: no reading`); continue; }
      const delta = this.settings.bindings[id].polarity * (raw - axis.reference) * 2 * Math.PI / COUNTS;
      values[i] = LegMirror.savedAngle(axis, this.coordinates[i]) + delta;
      lines.push(`${role}: ${(delta * 180 / Math.PI).toFixed(1)}° from its alignment pose`);
    }
    const key = values.join(',');
    if (!force && key === this.pending) return;
    this.pending = key; this.text = lines.join(' · ');
    this.solve(values);
  }
  /// Load a compiled gait into the shared Rust sampler (the one the hardware host uses).
  async loadGait(compiled, name) {
    if (!this.coordinates) await this.begin();
    this.gaitInfo = (await this.request('gait_load', {compiled, name})).info;
    return this.gaitInfo;
  }
  /// Joint pose at gait time `t`; with a governor, the governed (commanded)
  /// pose after `dt` s of playback at `scale` (`reset` restarts it).
  async sampleGait(t, dt = 0.02, scale = 1, reset = false) {
    const {values} = await this.request('gait_sample', {t, dt, scale, reset, governed: !!this.gaitInfo?.governor});
    return Object.fromEntries(this.gaitInfo.joints.map((j, i) => [j, values[i]]));
  }
  /// Show a gait pose (joint → rad) or clear it; `realLeg` keeps the bound leg on the encoders.
  setGait(pose, realLeg = true) { this.gait = pose; this.gaitRealLeg = realLeg; this.update(this.last?.state ?? {}, true); }
  /// Motor → CAD joint bindings for driving the real leg with a gait: aligned,
  /// taught, enabled motors only, with the joint angle each was aligned at.
  gaitBindings(state) {
    const out = [], skipped = [];
    for (const [id, role] of Object.entries(this.roles)) {
      const axis = state.calibration?.axes?.[id] ?? {}, joint = this.joint(id), c = this.coordinates?.find(x => x.joint === joint);
      const why = axis.disabled ? 'disabled' : axis.lower == null || axis.upper == null ? 'poses not taught' : axis.reference == null ? 'not aligned to the sim' : !c ? 'no CAD joint' : null;
      if (why) { skipped.push(`${role} (${why})`); continue; }
      out.push({id: Number(id), joint, polarity: this.settings.bindings[id].polarity, home_rad: LegMirror.savedAngle(axis, c)});
    }
    return {bindings: out, skipped};
  }
  async solve(values) {
    if (this.busy) { this.queued = values; return; }
    this.busy = true;
    try {
      const pose = await this.request('mirror_pose', {coordinates: values});
      window.robotViewer.show(pose.poses);
      const limits = pose.authored_limit_violations.filter(n => n.startsWith(this.settings.leg));
      this.status(this.text + (limits.length ? ` · Beyond CAD limit: ${limits.join(', ')}` : ''));
    } catch (e) { this.status(`${this.text} · Pose not solved: ${e.message}`); }
    finally { this.busy = false; if (this.queued) { const q = this.queued; this.queued = null; this.solve(q); } }
  }
}
