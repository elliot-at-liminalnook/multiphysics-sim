import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
const $ = id => document.getElementById(id);
const viewport = $('viewport');
const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(42, 1, .001, 100);
camera.up.set(0, 0, 1);
let renderer;
try { renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true }); }
catch (error) {
  $('status').textContent = '3D graphics could not start. Check browser graphics support, then reload.';
  $('overlay').hidden = false; $('overlay').classList.add('error'); $('cancel').hidden = true;
  throw error;
}
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
viewport.prepend(renderer.domElement);
const controls = new OrbitControls(camera, renderer.domElement);
controls.enableDamping = true;
scene.add(new THREE.HemisphereLight(0xc8e6ff, 0x526468, 2.4));
const light = new THREE.DirectionalLight(0xffffff, 3.5); light.position.set(.4, -.8, 2); scene.add(light);
const model = new THREE.Group(); scene.add(model);
const arrows = new THREE.Group(); scene.add(arrows);
let grid, selectionBox, meshes = new Map(), current, frame, playback, worker, epoch = 0;
let abort, playing = false, busy = false, inputs = [], values = [], tick = 0, replaySaved;
let lastDraw = performance.now(), simulatedWork = 0, wallWork = 0, selectedName;
let liveTimer, liveStartWall = 0, liveStartSim = 0;
const driveKeys = new Set();
const ray = new THREE.Raycaster();
const pointer = new THREE.Vector2();
new ResizeObserver(() => {
  const w = viewport.clientWidth, h = viewport.clientHeight;
  renderer.setSize(w, h, false); camera.aspect = w / h; camera.updateProjectionMatrix();
}).observe(viewport);

function status(message, error = false) {
  $('status').textContent = message; $('overlay').hidden = !message;
  $('overlay').classList.toggle('error', error);
}
function dispose(object) { object.traverse(o => { o.geometry?.dispose(); if (Array.isArray(o.material)) o.material.forEach(m => m.dispose()); else o.material?.dispose(); }); }
function workerClient() {
  const instance = new Worker('./worker.js', { type: 'module' });
  let serial = 0; const requests = new Map();
  function rejectAll(message) { for (const item of requests.values()) { clearTimeout(item.timer); item.reject(new Error(message)); } requests.clear(); }
  instance.onmessage = ({ data }) => { const item = requests.get(data.id); if (!item) return; clearTimeout(item.timer);
    if (data.progress) { item.timer = setTimeout(item.expire, 30000); item.progress?.(data.progress); return; }
    requests.delete(data.id); data.error ? item.reject(new Error(data.error)) : item.resolve(data.result); };
  instance.onerror = e => rejectAll(e.message || 'Simulation worker failed');
  return {
    request(type, data = {}, progress) { return new Promise((resolve, reject) => {
      const id = ++serial; const expire = () => { requests.delete(id); reject(new Error('Simulation took too long. Reset or choose another run.')); };
      const timer = setTimeout(expire, 30000);
      requests.set(id, { resolve, reject, timer, expire, progress }); instance.postMessage({ id, type, ...data });
    }); },
    close() { rejectAll('Operation cancelled'); instance.terminate(); }
  };
}
async function fetchData(path, signal) {
  const response = await fetch(path, { signal }); if (!response.ok) throw new Error(`Could not load ${path} (${response.status})`);
  return response.json();
}
function scheduleLive() {
  clearTimeout(liveTimer);
  if (!playing || playback || busy || !worker) return;
  // Simulation is paced by elapsed time, independent of display refresh. If a
  // solve falls behind, run the next held-action transition without skipping it.
  const delay = Math.max(0, (tick-liveStartSim)*1000-(performance.now()-liveStartWall));
  liveTimer = setTimeout(() => advanceLive(), delay);
}
function setPlaying(value) { playing = value; clearTimeout(liveTimer);
  if (!playing && driveKeys.size) {driveKeys.clear();applyDriveKeys();}
  if (playing) { liveStartWall = performance.now(); liveStartSim = tick; scheduleLive(); }
  $('play').textContent = playing ? 'Pause' : 'Play';
  $('execution-state').textContent = playing ? (playback ? 'Playing recorded physics' : 'Running physics in background…') : (busy ? 'Pausing after the current physics chunk…' : 'Paused'); }
function selectPart(name) {
  selectedName = name;
  for (const [n, mesh] of meshes) mesh.material.emissive.set(n === name ? 0x225c54 : 0x000000);
  if (selectionBox) { scene.remove(selectionBox); dispose(selectionBox); selectionBox = null; }
  if (meshes.has(name)) { selectionBox = new THREE.BoxHelper(meshes.get(name), 0x9ef9d7); scene.add(selectionBox); }
  $('selected').textContent = name || 'Whole model';
  $('selection-detail').textContent = name ? 'Highlighted component · fit it for a closer look.' : 'Click a component to highlight it.';
  $('fit-selected').disabled = !name;
  for (const b of $('parts').children) b.classList.toggle('selected', b.dataset.name === name);
}
function fit(object = model) {
  scene.updateMatrixWorld(true);
  const box = new THREE.Box3().setFromObject(object); if (box.isEmpty()) return;
  const center = box.getCenter(new THREE.Vector3()), size = box.getSize(new THREE.Vector3()).length();
  controls.target.copy(center); const distance = Math.max(.025, size * 1.4);
  camera.position.copy(center).add(new THREE.Vector3(1, -1.5, .9).normalize().multiplyScalar(distance));
  camera.near = Math.max(.00001, distance / 1000); camera.far = Math.max(10, distance * 100); camera.updateProjectionMatrix(); controls.update();
}
function buildModel(robot) {
  selectPart(null); for (const mesh of [...model.children]) { model.remove(mesh); dispose(mesh); } meshes.clear();
  if (grid) { scene.remove(grid); dispose(grid); }
  for (const link of robot.links) {
    const c = link.collision; if (!c?.vertices?.length || !c.triangles?.length) continue;
    const geometry = new THREE.BufferGeometry(); geometry.setAttribute('position', new THREE.Float32BufferAttribute(c.vertices.flat(), 3)); geometry.setIndex(c.triangles.flat()); geometry.computeVertexNormals();
    const color = /motor|servo|HX-/i.test(link.name) ? 0x54718a : /crosshead|curved|crank/i.test(link.name) ? 0x7dbda8 : 0xbfcbd3;
    const mesh = new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({ color, metalness: .15, roughness: .65, side: THREE.DoubleSide }));
    mesh.matrixAutoUpdate = false; mesh.name = link.name; model.add(mesh); meshes.set(link.name, mesh);
  }
  const initial = robot.links.map(l => ({ name: l.name, position_m: l.com, rotation: [[1,0,0],[0,1,0],[0,0,1]] }));
  applyPoses(initial); scene.updateMatrixWorld(true);
  const extent = new THREE.Box3().setFromObject(model).getSize(new THREE.Vector3()).length();
  grid = new THREE.GridHelper(Math.max(.3, extent * 2.5), 30, 0x5b7684, 0x2c414f); grid.rotation.x = Math.PI / 2; grid.position.z = robot.world.floor_z; scene.add(grid);
  $('parts').replaceChildren(); $('search').value = '';
  for (const name of meshes.keys()) { const b = document.createElement('button'); b.textContent = name; b.dataset.name = name; b.onclick = () => selectPart(name); $('parts').append(b); }
  $('source').textContent = robot.source.cad_sha256 ? `CAD ${robot.source.cad_sha256.slice(0, 12)} · ${robot.links.length} rigid links` : `${robot.links.length} CAD-derived links · ${robot.source.exported || 'fixture'}`;
}
function applyPoses(poses) {
  for (const p of poses) { const mesh = meshes.get(p.name); if (!mesh) continue; const r = p.rotation, v = p.position_m;
    mesh.matrix.set(r[0][0],r[0][1],r[0][2],v[0], r[1][0],r[1][1],r[1][2],v[1], r[2][0],r[2][1],r[2][2],v[2], 0,0,0,1); mesh.matrixWorldNeedsUpdate = true;
  }
}
function showTaskObservations(next) {
  const config = current.data.policy_contract?.task_observations?.config;
  const feedbackConfig = current.data.policy_contract?.body_feedback?.config;
  const pointConfig = current.data.policy_contract?.point_feedback?.config;
  $('task-observation-panel').hidden = !config && !feedbackConfig && !pointConfig;
  const box = $('task-observation-readings'); box.replaceChildren();
  if (!config && !feedbackConfig && !pointConfig) return;
  if (!$('task-observation-details').open) return;
  const observations = next.policy?.observations;
  if (!observations) { box.textContent = 'Waiting for the first controller sample.'; return; }
  const stamp = document.createElement('p'); stamp.className = 'muted'; stamp.textContent = `Controller sample: ${next.policy.time_s.toFixed(3)} s · vectors shown as x, y, z`; box.append(stamp);
  const vector = (name, scale = 1) => ['x','y','z'].map(axis => (observations[`${name}.${axis}`] * scale).toFixed(2)).join(', ');
  const add = (label, text) => { const row = document.createElement('p'); const title = document.createElement('strong'); title.textContent = label; row.append(title, document.createElement('br'), document.createTextNode(text)); box.append(row); };
  const points = next.policy?.point_feedback;
  if (points) {
    const mm = values => values.map(v => (1000*v).toFixed(2)).join(', ');
    add('Foot / point position feedback', `Reference time: ${points.reference_time_s.toFixed(3)} s · ideal world observations`);
    pointConfig.markers.forEach((m,i) => add(m.id + ' world tracking', `target (mm): ${mm(points.target_positions_world_m[i])} · actual (mm): ${mm(points.actual_positions_world_m[i])} · error (mm): ${mm(points.position_errors_world_m[i])} · activation: ${(points.activation[i]*100).toFixed(0)}%`));
    add('Bounded point suggestion', `Largest before policy gain: ${(Math.max(...points.correction_rad.map(Math.abs))*180/Math.PI).toFixed(3)}°`);
  }
  const feedback = next.policy?.body_feedback;
  if (feedback) {
    const millimetres = values => values.map(v => (1000*v).toFixed(2)).join(', ');
    add('Body position feedback', `Reference time: ${feedback.reference_time_s.toFixed(3)} s · world target (mm): ${millimetres(feedback.target_position_world_m)} · actual (mm): ${millimetres(feedback.actual_position_world_m)} · error (mm): ${millimetres(feedback.position_error_world_m)}`);
    add('Support used for correction', feedbackConfig.support_markers.map((m,i) => `${m.id}: ${(100*feedback.support_weights[i]).toFixed(0)}%`).join(' · '));
    add('Bounded joint suggestion', `Largest correction before policy gain: ${(Math.max(...feedback.correction_rad.map(Math.abs))*180/Math.PI).toFixed(2)}°`);
  }
  if (config) add('Body', `Gravity direction: ${vector('body.gravity_direction')} · velocity (m/s): ${vector('body.linear_velocity')} · angular velocity (rad/s): ${vector('body.angular_velocity')}`);
  for (const marker of config?.markers || []) {
    const prefix = `marker.${marker.id}`;
    add(marker.id, `Position (mm): ${vector(prefix+'.position',1000)} · relative velocity (mm/s): ${vector(prefix+'.velocity',1000)}` + (config.floor_forces ? ` · upward support (N): ${observations[prefix+'.floor_force_world.z'].toFixed(2)}` : ''));
  }
}
function showMotionProgress(next) {
  const step = next.policy?.step_reference?.reference;
  if (step) {
    const box=$('motion-progress');box.hidden=false;box.replaceChildren();
    const title=document.createElement('strong');
    const marker=current.data.policy_contract?.point_feedback?.config.markers[step.foot]?.id;
    title.textContent=next.done?'Episode ended · reset to continue':step.phase==='idle'?'Standing · ready for a command':step.phase==='hold'?'Settling initial stance':`${marker || 'Foot'} · ${step.phase}${step.waiting?' · waiting for support':''}`;
    const detail=document.createElement('p');detail.textContent=`Completed transfers: ${step.step}. Current transfer: ${(step.progress*100).toFixed(0)}%. Latched speed: ${(step.latched_twist[0]*1000).toFixed(2)} mm/s · turn: ${(step.latched_twist[2]*180/Math.PI).toFixed(3)}°/s. Changes apply at the next transfer.`;
    box.append(title,detail);return;
  }
  const p = next.motion_progress, box = $('motion-progress'); box.hidden = !p;
  if (!p) return;
  const labels = {initial:'Preparing motion',following_reference:'Following the plan',waiting_for_condition:'Waiting for sustained foot support',condition_qualified:'Foot support qualified',complete:'Planned motion complete',timed_out:'Support checkpoint timed out'};
  box.replaceChildren();
  const heading = document.createElement('strong'); heading.textContent = labels[p.phase] || p.phase;
  const detail = document.createElement('p'); detail.textContent = `Plan: ${p.reference_time_s.toFixed(3)} / ${p.duration_s.toFixed(3)} s. Support observed for ${(p.qualified_duration_s*1000).toFixed(0)} / ${(p.required_qualification_s*1000).toFixed(0)} ms. Waiting: ${(p.paused_duration_s*1000).toFixed(0)} / ${(p.maximum_pause_s*1000).toFixed(0)} ms.`;
  box.append(heading, detail);
}
function showFrame(next) {
  frame = next; tick = next.time_s; applyPoses(next.poses); showTaskObservations(next); showMotionProgress(next);
  const load=$('world-load-readout');
  if(load&&next.environment_load){
    const w=next.environment_load,active=[...w.force_world_n,...w.moment_world_nm].some(v=>v!==0);
    load.textContent=`${active?'Push active':'Push inactive'} · world force [${w.force_world_n.map(v=>v.toFixed(3)).join(', ')}] N · moment [${w.moment_world_nm.map(v=>v.toFixed(3)).join(', ')}] N·m about the body's center of mass.`;
  }
  const neural=$('neural-residuals');
  if(neural?.open)for(const row of neural.querySelectorAll('[data-target]')){
    row.textContent=`${row.dataset.target.replace(/\.target$/,'')}: ${(next.policy?.neural_residual?.[row.dataset.target]??0).toFixed(6)} rad`;
  }
  const learning=next.learning, panel=$('learning-progress');panel.hidden=!learning;
  if (learning) {
    const state=learning.terminated?'Task bound reached':learning.truncated?'Time limit reached':'Episode in progress';
    panel.textContent=`Teacher environment · ${state}. Last ${(learning.elapsed_s*1000).toFixed(0)} ms score: ${learning.reward.toFixed(6)}. ${learning.observations.length} ideal observations; not hardware sensor readings.${learning.termination_reasons.length?' '+learning.termination_reasons.join('; '):''}`;
  }
  $('time').textContent = `${tick.toFixed(3)} s`; $('sim-time').textContent = `${tick.toFixed(3)} s`; $('timeline').value = tick;
  $('execution-state').textContent = next.error ? 'Experiment stopped with an error' : learning?.terminated ? 'Task bound reached' : learning?.truncated ? 'Episode time limit reached' : next.done ? 'Experiment complete' : playing ? (playback ? 'Playing recorded physics' : `Running · ${next.completed_steps ?? ''}${next.requested_steps ? ' / '+next.requested_steps+' physics steps' : ''}`) : 'Paused';
  const contacts = next.contacts || []; $('contact-count').textContent = String(contacts.length);
  for (const a of [...arrows.children]) { arrows.remove(a); dispose(a); }
  for (const c of contacts) { const p = c.point_m, f = c.force_n; if (!p || !f) continue; const force = new THREE.Vector3(...f), magnitude = force.length(); if (magnitude < 1e-7) continue;
    arrows.add(new THREE.ArrowHelper(force.normalize(), new THREE.Vector3(...p), Math.min(.10, magnitude * .004), c.other == null ? 0x8cf1ce : 0xffa785, .009, .005));
  }
  const readings = next.servo_targets_rad?.map((target, i) => ({ name: current.data.coordinate_names[i].replace('joint.', ''), reference: next.reference_targets_rad?.[i], target, actual: next.joint_positions[current.data.joint_indices[i]] })) ||
    next.joint_positions?.map((actual, i) => ({ name: `Joint ${i + 1}`, actual, target: next.telemetry?.actuators?.[i] }));
  $('readings-label').textContent = next.reference_targets_rad ? 'Plan → motor target → actual' : 'Requested → actual';
  $('joint-readings').replaceChildren();
  for (const r of readings || []) { const row = document.createElement('div'); row.className = 'reading'; const label = document.createElement('span'); label.textContent = r.name; label.title = r.name;
    const value = document.createElement('span'); value.textContent = `${r.reference == null ? '' : (r.reference * 180 / Math.PI).toFixed(1) + ' → '}${r.target == null ? '—' : (r.target * 180 / Math.PI).toFixed(1)} → ${(r.actual * 180 / Math.PI).toFixed(1)}°`; row.append(label, value); $('joint-readings').append(row); }
}
function restoreInputs(restored) {
  if (!restored || restored.length !== inputs.length) return;
  for (const [i, slider] of [...$('inputs').querySelectorAll('input')].entries()) {
    slider.value = restored[i]; slider.dispatchEvent(new Event('input'));
  }
}
function makeInputs(channels) {
  inputs = channels; values = channels.map(c => c.initial); $('inputs').replaceChildren();
  driveKeys.clear();const drive=channels.length&&current.data?.policy_contract?.step_reference?.config;
  $('teleop').hidden=!drive;$('teleop').replaceChildren();
  if (drive) {
    const help=document.createElement('p');help.textContent='Press Play, then hold W/S to move, A/D to turn. Release to request a stop after the current foot transfer.';$('teleop').append(help);
    for (const [key,label] of [['w','W · Forward'],['a','A · Left'],['s','S · Back'],['d','D · Right']]) {
      const button=document.createElement('button');button.textContent=label;button.dataset.driveKey=key;
      button.onpointerdown=e=>{e.preventDefault();button.setPointerCapture(e.pointerId);driveKeys.add(key);applyDriveKeys();};
      const release=()=>{driveKeys.delete(key);applyDriveKeys();};button.onpointerup=release;button.onpointercancel=release;button.onlostpointercapture=release;
      $('teleop').append(button);
    }
    const stop=document.createElement('button');stop.id='stop-motion';stop.textContent='Stop motion';stop.onclick=()=>{driveKeys.clear();applyDriveKeys();};$('teleop').append(stop);
  }
  const residualStart=channels.findIndex(c=>c.name.startsWith('residual.'));
  let residualGroup;
  if(residualStart>=0&&channels.slice(residualStart).every(c=>c.name.startsWith('residual.'))){
    residualGroup=document.createElement('details');residualGroup.id='residual-inputs';
    residualGroup.hidden=Boolean(current?.data?.config?.policy?.neural_residual);
    const summary=document.createElement('summary');summary.textContent=`Motor corrections (${channels.length-residualStart})`;residualGroup.append(summary);
    const help=document.createElement('p');help.textContent='Angle offsets added to the crawl controller. Zero uses the baseline. Command limits still apply.';residualGroup.append(help);
    const clear=document.createElement('button');clear.id='clear-residuals';clear.textContent='Clear motor corrections';
    clear.onclick=()=>{for(const slider of residualGroup.querySelectorAll('input')){slider.value=0;slider.dispatchEvent(new Event('input'));}};residualGroup.append(clear);
  }
  channels.forEach((c, i) => { const label = document.createElement('label'), output = document.createElement('span'), slider = document.createElement('input');
    const display=()=>c.kind==='LinearVelocity'?`${(values[i]*1000).toFixed(2)} mm/s`:c.kind==='AngularVelocity'?`${(values[i]*180/Math.PI).toFixed(3)}°/s`:`${values[i].toFixed(c.kind==='Angle'&&c.upper-c.lower<=.1?4:2)} ${c.kind==='Angle'?'rad':''}`;
    output.textContent = `${c.name}: ${display()}`;
    slider.type = 'range'; slider.min = c.lower; slider.max = c.upper; slider.step = (c.upper-c.lower)/200 || 1; slider.disabled=c.lower===c.upper; slider.value = c.initial; slider.setAttribute('aria-label', c.name);
    slider.oninput = () => { values[i] = Number(slider.value); output.textContent = `${c.name}: ${display()}`; };
    label.append(output, slider);
    if(residualGroup&&i>=residualStart){if(i===residualStart)$('inputs').append(residualGroup);residualGroup.append(label);}
    else $('inputs').append(label);
  });
  const network=current?.data?.config?.policy?.neural_residual;
  const loads=current?.data?.config?.world_loads;
  if(loads){
    const panel=document.createElement('section');panel.id='world-load-panel';
    const title=document.createElement('strong');title.textContent='Scheduled body pushes';panel.append(title);
    const scope=document.createElement('p');scope.textContent=`${loads.base_link} · experimental disturbance. Maximum combined force ${loads.maximum_force_n} N; moment ${loads.maximum_moment_nm} N·m.`;panel.append(scope);
    for(const p of loads.pulses){const item=document.createElement('p');item.textContent=`${p.name}: ${p.start_s.toFixed(2)}–${(p.start_s+p.duration_s).toFixed(2)} s of simulation time.`;panel.append(item);}
    const value=document.createElement('p');value.id='world-load-readout';value.textContent='Push inactive';panel.append(value);$('inputs').append(panel);
  }
  if(network&&channels.length){
    const details=document.createElement('details');details.id='neural-residuals';
    const title=document.createElement('summary');title.textContent='Learned motor corrections';details.append(title);
    const note=document.createElement('p');note.textContent='Rust network outputs added to baseline feedback. These readouts are controlled by the policy; WASD requests the motion task.';details.append(note);
    for(const output of network.outputs){const row=document.createElement('p');row.dataset.target=output.target;row.textContent=`${output.target.replace(/\.target$/,'')}: 0.000000 rad`;details.append(row);}
    details.addEventListener('toggle',()=>{if(details.open&&frame)for(const row of details.querySelectorAll('[data-target]'))row.textContent=`${row.dataset.target.replace(/\.target$/,'')}: ${(frame.policy?.neural_residual?.[row.dataset.target]??0).toFixed(6)} rad`;});
    $('inputs').append(details);
  }
}
async function loadPreset(id) {
  const token = ++epoch; abort?.abort(); worker?.close(); worker = null; abort = new AbortController();
  setPlaying(false); busy = false; replaySaved = null; simulatedWork = wallWork = 0; playback = null;
  for (const id of ['play','step','reset','timeline','download','replay']) $(id).disabled = true;
  $('cancel').hidden = false; status('Loading model and controller…');
  try {
    const preset = catalog.presets.find(p => p.id === id); const data = await fetchData(preset.path, abort.signal); if (token !== epoch) return;
    current = { ...preset, data }; $('description').textContent = preset.description; $('readiness').textContent = preset.readiness; $('readiness').dataset.state = preset.readiness_state || 'experimental'; $('evidence').textContent = preset.evidence;
    $('mode').textContent = preset.mode !== 'recorded' ? 'LIVE · Rust / WASM' : 'RECORDED PHYSICS'; $('view-title').textContent = preset.label;
    buildModel(data.robot || data.scene.robot); makeInputs([]);
    if (preset.mode !== 'recorded') {
      worker = workerClient(); const result = await worker.request('load', { scene: data.scene || data, config: data.config, task: data.task, seed: 0 }); if (token !== epoch) return;
      if (result.metadata) Object.assign(current.data, result.metadata);
      makeInputs(result.inputs); showFrame(result.frame); $('timeline').max = result.metadata ? result.metadata.steps * result.metadata.step_s : data.duration_s;
      $('input-help').textContent = result.metadata?.environment_contract ? `Each command is held for ${result.metadata.environment_contract.period_s*1000} ms of simulation time. The selected controller and actuator profile determine the motor response. The score measures endpoint joint tracking; it is not yet a walking objective. Saving and replay preserve the task and command sequence.` : result.metadata?.policy_contract ? 'Rhai reads ideal simulated joint state and sends motor targets at its declared sampling rate. Adjust the commands above; save and replay preserve when they changed. Hardware sensor bindings and walking commands are not yet available.' : preset.mode === 'embedded' ? 'The Rust servo controller executes this experiment live. Pause and reset are available; this preset does not yet declare WASD walking commands.' : 'Use the position slider while running. This fixture has no walking command; WASD locomotion is unavailable.';
      if (current.data.policy_contract?.step_reference) $('input-help').textContent+=' Motion requests are latched at foot-transfer boundaries. Releasing a key finishes the current transfer before standing; this provisional crawl is deliberately slow.';
      $('performance').textContent = 'Waiting for physics'; $('speed').disabled = true;
    } else {
      playback = data.frames; showFrame(playback[0]); $('timeline').max = playback.at(-1).time_s; $('timeline').disabled = false; $('speed').disabled = false;
      $('input-help').textContent = 'Recorded motor execution. Scrub to inspect it, or choose the live lift experiment to execute its controller.';
      $('performance').textContent = `${(data.simulated_s/data.stepping_wall_s).toFixed(3)}× recorded`;
    }
    fit(); $('play').disabled = $('reset').disabled = $('download').disabled = false; $('step').disabled=Boolean(playback); status('');
  } catch (error) { if (token === epoch) { setPlaying(false); status(error.message, true); $('reset').disabled = false; } }
  finally { if (token === epoch) $('cancel').hidden = true; }
}
async function advanceLive(single=false) {
  if (busy || (!playing && !single) || !worker) return; busy = true; const token = epoch, before = performance.now(), old = tick;
  try { const next = await worker.request('step', { action: values }); if (token !== epoch) return; showFrame(next);
    wallWork += (performance.now()-before)/1000; simulatedWork += tick-old;
    const liveRate = (tick-liveStartSim)/((performance.now()-liveStartWall)/1000);
    $('performance').textContent = single ? `${(simulatedWork/wallWork).toFixed(2)}× processing` : `${liveRate.toFixed(2)}× live`;
    $('performance').title = `Worker and scene-update throughput: ${(simulatedWork/wallWork).toFixed(2)}×. Live rate also includes scheduling time.`;
    if (next.done || next.error) { setPlaying(false); showFrame(next); if (next.error) status(next.error, true); }
  } catch (e) { if (token === epoch) { setPlaying(false); status(e.message, true); } }
  finally { if (token === epoch) { busy = false; scheduleLive(); } }
}
function replayAt(time) {
  if (!playback) return; let lo = 0, hi = playback.length - 1;
  while (lo < hi) { const mid = Math.ceil((lo + hi)/2); if (playback[mid].time_s <= time) lo = mid; else hi = mid-1; }
  showFrame(playback[lo]);
}
let replayClock = 0;
renderer.setAnimationLoop(now => {
  const elapsed = Math.min((now-lastDraw)/1000, .1); lastDraw = now;
  if (playing && playback) { replayClock += elapsed * Number($('speed').value); replayAt(replayClock); if (replayClock >= playback.at(-1).time_s) setPlaying(false); }
  controls.update(); scene.updateMatrixWorld(true); selectionBox?.update(); arrows.visible = $('contacts').checked; renderer.render(scene,camera);
});
$('fit').onclick = () => fit(); $('fit-selected').onclick = () => { if (meshes.has(selectedName)) fit(meshes.get(selectedName)); };
$('search').oninput = () => { for (const b of $('parts').children) b.hidden = !b.dataset.name.toLowerCase().includes($('search').value.toLowerCase()); };
$('play').onclick = () => { if (!playing && tick >= Number($('timeline').max)-1e-10) { if (playback) { replayAt(0); } else { status('Reset the completed run before playing again.'); return; } } replayClock = tick; setPlaying(!playing); };
$('step').onclick = async () => { setPlaying(false); if(frame?.done){status('Reset the ended episode before stepping again.');return;} $('step').disabled=true;await advanceLive(true);if(worker&&!playback)$('step').disabled=false; };
$('timeline').oninput = () => { setPlaying(false); replayAt(Number($('timeline').value)); };
$('reset').onclick = () => loadPreset($('preset').value);
$('cancel').onclick = () => { epoch++; abort?.abort(); worker?.close(); worker = null; busy = false; setPlaying(false); status('Cancelled. Choose an experiment or reset to try again.'); $('cancel').hidden = true; $('reset').disabled = false; };
$('download').onclick = async () => {
  const token = epoch, id = current.id, recorded = Boolean(playback);
  try { const data = recorded ? current.data : await worker.request('recording'); if (token !== epoch) return;
    if (!recorded) { replaySaved = data; $('replay').disabled = false; }
    const url = URL.createObjectURL(new Blob([JSON.stringify(data)], { type: 'application/json' })); const a = document.createElement('a'); a.href = url; a.download = `${id}.json`; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  } catch (e) { if (token === epoch) status(e.message, true); }
};
$('replay').onclick = async () => { setPlaying(false); if (!replaySaved || !worker) return; status('Re-executing recorded inputs…'); const token = epoch;
  $('play').disabled = $('step').disabled = $('replay').disabled = true; $('cancel').hidden = false;
  try { const next = await worker.request('replay', { recording: replaySaved }, p => status(`Replaying physics: ${p.completed_steps} / ${p.total_steps} steps…`)); if (token === epoch) { showFrame(next); restoreInputs(next.policy_inputs); status(next.error || '', Boolean(next.error)); } } catch (e) { if (token === epoch) status(e.message, true); }
  finally { if (token === epoch) { $('play').disabled = $('step').disabled = $('replay').disabled = false; $('cancel').hidden = true; } }
};
let dragStart;
renderer.domElement.addEventListener('pointerdown', e => { dragStart = [e.clientX,e.clientY]; });
renderer.domElement.addEventListener('pointerup', e => { if (!dragStart || Math.hypot(e.clientX-dragStart[0],e.clientY-dragStart[1])>4) return;
  const b = renderer.domElement.getBoundingClientRect(); pointer.set((e.clientX-b.left)/b.width*2-1,-(e.clientY-b.top)/b.height*2+1); ray.setFromCamera(pointer,camera); selectPart(ray.intersectObjects([...meshes.values()])[0]?.object.name || null);
});
function applyDriveKeys() {
  const drive=current?.data?.policy_contract?.step_reference?.config;if(!drive)return;
  const directions=[Number(driveKeys.has('w'))-Number(driveKeys.has('s')),0,Number(driveKeys.has('a'))-Number(driveKeys.has('d'))];
  drive.command_channels.forEach((name,axis)=>{
    const i=inputs.findIndex(c=>c.name===name);if(i<0)return;const c=inputs[i];
    const slider=$('inputs').querySelectorAll('input')[i];slider.value=directions[axis]>0?c.upper:directions[axis]<0?c.lower:0;slider.dispatchEvent(new Event('input'));
  });
  for(const b of $('teleop').querySelectorAll('[data-drive-key]'))b.setAttribute('aria-pressed',String(driveKeys.has(b.dataset.driveKey)));
}
window.addEventListener('keydown', e => { if (/INPUT|SELECT|TEXTAREA/.test(e.target.tagName)||e.target.isContentEditable) return;
  const key=e.key.toLowerCase();if('wasd'.includes(key)&&key.length===1&&current?.data?.policy_contract?.step_reference){e.preventDefault();driveKeys.add(key);applyDriveKeys();return;}
  if (key==='f') fit(selectedName ? meshes.get(selectedName) : model); if (e.code==='Space') {e.preventDefault(); if (!$('play').disabled) $('play').click();} });
window.addEventListener('keyup',e=>{const key=e.key.toLowerCase();if(driveKeys.delete(key)){e.preventDefault();applyDriveKeys();}});
window.addEventListener('blur',()=>{driveKeys.clear();applyDriveKeys();});
$('task-observation-details').addEventListener('toggle',()=>{if(frame)showTaskObservations(frame);});
let catalog;
try { catalog = await fetchData('catalog.json'); for (const p of catalog.presets) { const option = document.createElement('option'); option.value = p.id; option.textContent = p.label; $('preset').append(option); }
  $('preset').disabled = false; $('preset').onchange = () => loadPreset($('preset').value);
  const requested=new URL(location.href).searchParams.get('preset');
  const initial=catalog.presets.find(p=>p.id===requested)?.id||catalog.presets[0].id;
  $('preset').value=initial;
  await loadPreset(initial);
} catch(e) {status(e.message, true);}
