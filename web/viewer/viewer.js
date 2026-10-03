import {installHardwareSync} from './hardware-sync.mjs';
import {motionCommandConfig,motionHeartbeatIndex,nextMotionAction,boundedInputValue,driveMotionValues} from "./motion-commands.mjs";
import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { installLeaderboard } from './leaderboard.js';
import { installVideoExport } from './video-export.js';
import { decodeWorkerResult } from './worker-message.mjs';
import { createDriveInput, loadDriveBindings, isTextField, isTextEntry, isChord, gamepadSnapshot, drivePeriodsPerChunk, errorText } from './drive-input.mjs';
import { createDrivePanel } from './drive-panel.mjs';
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
// Read-only diagnostics distinguish actual drawing from display scheduling.
export function renderedFrameCount() { return renderer.info.render.frame; }
let lastRenderedFrame = null;
// This records WebGL submission, not monitor presentation or a physical response.
export function renderedFrameInfo() { return lastRenderedFrame && structuredClone(lastRenderedFrame); }
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
// Drive-profile presets (mode "drive"): {input, panel, periods, replaying, replayPending}, set once loaded.
// replayPending is set and cleared only by replayDrive, so a live chunk that
// resolves after a replay started cannot re-enable device input.
// Other presets keep the motion-commands WASD path below unchanged.
let drive = null, driveWasmReady = null;
const DRIVE_REPLAY_CHUNK_PERIODS = 200;
const videoCapture = installVideoExport(renderer.domElement, $('video'), () => current?.id || 'robot');
let drawNeeded = true;
// While a calibration mirror is active, measured geometry replaces simulated frames.
let mirrorActive = false, mirrorLinks = new Set();
let lastSubmittedAt = -Infinity;
controls.addEventListener('change', () => { drawNeeded = true; });
const driveKeys = new Set();
const ray = new THREE.Raycaster();
const pointer = new THREE.Vector2();
new ResizeObserver(() => {
  const w = viewport.clientWidth, h = viewport.clientHeight;
  renderer.setSize(w, h, false); camera.aspect = w / h; camera.updateProjectionMatrix();
  drawNeeded = true;
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
    requests.delete(data.id);
    try { data.error ? item.reject(new Error(data.error)) : item.resolve(decodeWorkerResult(data)); }
    catch (error) { item.reject(error); }
  };
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
async function fetchData(path, signal, expectedSha) {
  const response = await fetch(path, { signal }); if (!response.ok) throw new Error(`Could not load ${path} (${response.status})`);
  if (expectedSha) {
    const bytes = await response.arrayBuffer();
    const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))].map(b => b.toString(16).padStart(2, '0')).join('');
    if (digest !== expectedSha) throw new Error('Tested recipe integrity mismatch. Rebuild the viewer from its evaluation artifacts.');
    return JSON.parse(new TextDecoder().decode(bytes));
  }
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
function setPlaying(value) { if(value&&mirrorActive)return; if(!value)hardwareSync.stop('Simulation paused'); playing = value; clearTimeout(liveTimer);
  if (!playing && driveKeys.size) {driveKeys.clear();applyDriveKeys();}
  if (!playing && drive) { sendDrive(drive.input.stop()); pauseDrive(); }
  if (playing) { liveStartWall = performance.now(); liveStartSim = tick; scheduleLive(); }
  $('play').textContent = playing ? 'Pause' : 'Play';
  $('execution-state').textContent = playing ? (playback ? 'Playing recorded physics' : 'Running physics in background…') : (busy ? 'Pausing after the current physics chunk…' : 'Paused'); }
function selectPart(name) {
  drawNeeded = true;
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
  drawNeeded = true;
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
  const gridSize = Math.max(.3, extent * 2.5, current.data.view_grid_size_m || 0);
  grid = new THREE.GridHelper(gridSize, Math.max(30, Math.round(gridSize / .1)), 0x5b7684, 0x2c414f); grid.rotation.x = Math.PI / 2; grid.position.z = robot.world.floor_z; scene.add(grid);
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
    const prelift=current.data.policy_contract?.step_reference?.config.sequence.update_command_before_lift;
    title.textContent=next.done?'Episode ended · reset to continue':step.phase==='idle'?'Standing · ready for a command':step.phase==='hold'?'Settling initial stance':step.phase==='recenter'?`Returning to standing · lift canceled${step.waiting?' · waiting for all feet to support the body':''}`:`${marker || 'Foot'} · ${step.phase}${step.waiting?' · waiting for support':''}`;
    const detail=document.createElement('p');detail.textContent=`Completed transfers: ${step.step}. Current transfer: ${(step.progress*100).toFixed(0)}%. Latched speed: ${(step.latched_twist[0]*1000).toFixed(2)} mm/s · turn: ${(step.latched_twist[2]*180/Math.PI).toFixed(3)}°/s. ${prelift?'New requests are checked again before lift-off. Airborne steps and reversals finish the current transfer.':'Changes apply at the next transfer.'}`;
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
  if (mirrorActive) return;
  drawNeeded = true;
  if ($('follow').checked && current.data.follow_link && frame) {
    const old = frame.poses.find(p => p.name === current.data.follow_link);
    const updated = next.poses.find(p => p.name === current.data.follow_link);
    if (old && updated) {
      const delta = new THREE.Vector3(...updated.position_m).sub(new THREE.Vector3(...old.position_m));
      camera.position.add(delta); controls.target.add(delta);
    }
  }
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
  const travel=$('travel-speed'),speed=learning?.speed;travel.hidden=!speed;
  if(speed)travel.textContent=`Actual net travel: ${(frame.time_s>0?speed.net_distance_m/frame.time_s:0).toFixed(3)} m/s · ${speed.net_distance_m.toFixed(3)} m since reset${speed.fallen?' · FALL DETECTED':''}. Mean over elapsed simulation time.`;
  const walking=learning?.walking,overlay=$('walking-overlay');overlay.hidden=!walking;
  if(walking)overlay.textContent=`${walking.qualified_steps} qualified · ${walking.failed_steps} failed · body error ${(Math.hypot(...walking.body_error_world_m)*1000).toFixed(2)} mm${walking.heading?' · heading '+(walking.heading.error_rad*180/Math.PI).toFixed(3)+'°':''}`;
  if (learning) {
    const state=learning.terminated?'Task bound reached':learning.truncated?'Time limit reached':'Episode in progress';
    const w=learning.walking,walking=w?` Qualified steps: ${w.qualified_steps}; failed: ${w.failed_steps}. Body reference error: ${(Math.hypot(...w.body_error_world_m)*1000).toFixed(2)} mm.${w.outcome?' Last step '+w.outcome.step+': '+(w.outcome.passed?'qualified':'failed')+'.':''}`:'';
    const heading=w?.heading?` Heading error: ${(w.heading.error_rad*180/Math.PI).toFixed(3)}°; heading score: ${w.heading.reward.toFixed(6)}.`:'';
    panel.textContent=`Learning environment · ${state}. Last ${(learning.elapsed_s*1000).toFixed(0)} ms score: ${learning.reward.toFixed(6)}. ${learning.observations.length} ideal observations; not hardware sensor readings.${walking}${heading}${learning.termination_reasons.length?' '+learning.termination_reasons.join('; '):''}`;
  }
  $('time').textContent = `${tick.toFixed(3)} s`; $('sim-time').textContent = `${tick.toFixed(3)} s`; $('timeline').value = tick;
  $('execution-state').textContent = next.error ? 'Experiment stopped with an error' : learning?.terminated ? 'Task bound reached' : learning?.truncated ? 'Episode time limit reached' : next.done ? 'Experiment complete' : playing ? (playback ? 'Playing recorded physics' : `Running · ${next.completed_steps ?? ''}${next.requested_steps ? ' / '+next.requested_steps+' physics steps' : ''}`) : 'Paused';
  const contacts = next.contacts || []; $('contact-count').textContent = String(contacts.length);
  let activeArrows=0;
  for (const c of contacts) { const p = c.point_m, f = c.force_n; if (!p || !f) continue; const force = new THREE.Vector3(...f), magnitude = force.length(); if (magnitude < 1e-7) continue;
    let arrow=arrows.children[activeArrows++];
    if(!arrow){arrow=new THREE.ArrowHelper();arrows.add(arrow);}
    arrow.visible=true;arrow.position.set(...p);arrow.setDirection(force.normalize());
    arrow.setLength(Math.min(.10,magnitude*.004),.009,.005);arrow.setColor(c.other==null?0x8cf1ce:0xffa785);
  }
  for(let i=activeArrows;i<arrows.children.length;i++)arrows.children[i].visible=false;
  const readings = next.servo_targets_rad?.map((target, i) => ({ name: (current.data.coordinate_names?.[i] ?? `Coordinate ${i + 1}`).replace('joint.', ''), reference: next.reference_targets_rad?.[i], target, actual: next.joint_positions[current.data.joint_indices[i]] })) ||
    next.joint_positions?.map((actual, i) => ({ name: `Joint ${i + 1}`, actual, target: next.telemetry?.actuators?.[i] }));
  $('readings-label').textContent = next.reference_targets_rad ? 'Plan → motor target → actual' : 'Requested → actual';
  const readingPanel=$('joint-readings');let readingIndex=0;
  for (const r of readings || []) { let row=readingPanel.children[readingIndex++];
    if(!row){row=document.createElement('div');row.className='reading';row.append(document.createElement('span'),document.createElement('span'));readingPanel.append(row);}
    const [label,value]=row.children;label.textContent=r.name;label.title=r.name;
    value.textContent = `${r.reference == null ? '' : (r.reference * 180 / Math.PI).toFixed(1) + ' → '}${r.target == null ? '—' : (r.target * 180 / Math.PI).toFixed(1)} → ${(r.actual * 180 / Math.PI).toFixed(1)}°`; }
  while(readingPanel.children.length>readingIndex)readingPanel.lastElementChild.remove();
}
function restoreInputs(restored) {
  if (!restored || restored.length !== inputs.length) return;
  for (const [i, slider] of [...$('inputs').querySelectorAll('input')].entries()) {
    slider.value = restored[i]; slider.dispatchEvent(new Event('input'));
  }
}
function makeInputs(channels) {
  inputs = channels; values = channels.map(c => c.initial); $('inputs').replaceChildren();
  const heartbeat=channels.length?motionHeartbeatIndex(current,channels):-1;
  driveKeys.clear();const drive=channels.length&&motionCommandConfig(current,channels);
  $('teleop').hidden=!drive;$('teleop').replaceChildren();
  if (drive) {
    const help=document.createElement('p');help.textContent='Press Play, then hold W/S to move, A/D to turn. '+(drive.sequence.update_command_before_lift?'Release to request a stop. This controller can cancel a lift before it starts.':'Release to request a stop after the current foot transfer.');$('teleop').append(help);
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
  if(residualStart>=0&&channels.slice(residualStart).every((c,i)=>c.name.startsWith('residual.')||i+residualStart===heartbeat)){
    residualGroup=document.createElement('details');residualGroup.id='residual-inputs';
    residualGroup.hidden=Boolean(current?.data?.config?.policy?.neural_residual);
    const summary=document.createElement('summary');summary.textContent=`Motor corrections (${channels.filter(c=>c.name.startsWith('residual.')).length})`;residualGroup.append(summary);
    const help=document.createElement('p');help.textContent='Angle offsets added to the crawl controller. Zero uses the baseline. Command limits still apply.';residualGroup.append(help);
    const clear=document.createElement('button');clear.id='clear-residuals';clear.textContent='Clear motor corrections';
    clear.onclick=()=>{for(const slider of residualGroup.querySelectorAll('input')){slider.value=0;slider.dispatchEvent(new Event('input'));}};residualGroup.append(clear);
  }
  channels.forEach((c, i) => { const label = document.createElement('label'), output = document.createElement('span'), slider = document.createElement('input');
    const display=()=>c.kind==='LinearVelocity'?`${(values[i]*1000).toFixed(2)} mm/s`:c.kind==='AngularVelocity'?`${(values[i]*180/Math.PI).toFixed(3)}°/s`:`${values[i].toFixed(c.kind==='Angle'&&c.upper-c.lower<=.1?4:2)} ${c.kind==='Angle'?'rad':''}`;
    output.textContent = `${c.name}: ${display()}`;
    slider.type = 'range'; slider.min = c.lower; slider.max = c.upper; slider.step = (c.upper-c.lower)/200 || 1; slider.disabled=c.lower===c.upper; slider.value = c.initial; slider.setAttribute('aria-label', c.name);
    // Keep typed command values authoritative: HTML range controls can round
    // awkward decimal endpoints just outside the declared runtime bounds.
    slider.setCommandValue = value => { values[i]=boundedInputValue(c,value); slider.value=values[i]; output.textContent=`${c.name}: ${display()}`; };
    slider.oninput = () => slider.setCommandValue(Number(slider.value));
    label.append(output, slider);
    if(i===heartbeat){label.hidden=true;slider.step=1;slider.disabled=true;}
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
// A second Rust/WASM instance on the page thread runs only the pure binding and
// device functions, so stops and action edges are decided synchronously and are
// never queued behind a physics chunk in the worker.
function driveWasm() {
  driveWasmReady ??= import('./sim_web.js').then(async wasm => { await wasm.default(); return wasm; })
    .catch(error => { driveWasmReady = null; throw error; });
  return driveWasmReady;
}
async function fetchText(url, signal) {
  const response = await fetch(url, { signal }); if (!response.ok) throw new Error(`Could not load ${url.pathname} (${response.status})`);
  return response.text();
}
// Drive-profile preset: the page fetches text, Rust parses and builds everything.
async function loadDrive(preset, data, token) {
  if (data.kind !== 'drive_files' || !data.model?.url || !data.binding?.url) throw new Error(`${preset.path}: not a packaged drive preset; rebuild with web/build-viewer.mjs`);
  const signal = abort.signal, modelUrl = new URL(data.model.url, location.href), bindingUrl = new URL(data.binding.url, location.href);
  status('Loading the model, binding and Rust input functions…');
  const [modelText, bindingText, wasm] = await Promise.all([fetchText(modelUrl, signal), fetchText(bindingUrl, signal), driveWasm()]);
  if (token !== epoch) return false;
  worker = workerClient();
  const listed = await worker.request('drive_files', { binding_path: data.binding.path, binding_text: bindingText }); if (token !== epoch) return false;
  const files = {};
  for (const rel of listed) { files[rel] = await fetchText(new URL(rel, bindingUrl), signal); if (token !== epoch) return false; }
  status('Building the drive scene in Rust…');
  const driveJson = await worker.request('drive_build', { model_path: data.model.path, model_text: modelText, binding_path: data.binding.path, binding_text: bindingText, files }); if (token !== epoch) return false;
  const built = JSON.parse(driveJson);
  const loaded = await worker.request('drive_load', { drive_json: driveJson, seed: data.seed ?? 0 }); if (token !== epoch) return false;
  const metadata = loaded.metadata;
  if (!Array.isArray(metadata?.limits?.supported) || metadata.limits.supported.length !== 3) throw new Error('DriveSimulation metadata: limits.supported must list the three axes (forward, lateral, yaw)');
  Object.assign(current.data, { scene: built.scene, coordinate_names: metadata.coordinate_names ?? [], joint_indices: metadata.joint_indices ?? [],
    follow_link: preset.follow_link, view_grid_size_m: preset.view_grid_size_m });
  buildModel(built.scene.robot); makeInputs([]);
  const bindings = loadDriveBindings({ defaultBindings: wasm.default_drive_bindings, validateBindings: wasm.validate_drive_bindings,
    storage: { getItem: key => localStorage.getItem(key) } });
  const input = createDriveInput({ driveDeviceAxes: wasm.drive_device_axes, bindings: bindings.bindings, supported: metadata.limits.supported });
  // The panel's buttons go through the input state machine (zero first, then disarm), like a bound action.
  const panel = createDrivePanel($('teleop'), { drive: built, metadata, bindings,
    onAction: name => { if (drive) sendDrive(name === 'stop' ? drive.input.stop() : drive.input.action(name)); } });
  drive = { input, panel, periods: drivePeriodsPerChunk(metadata.period_s), replaying: false, replayPending: false };
  showFrame(loaded.frame); panel.status(loaded.frame.drive);
  $('timeline').max = built.scene.duration_s;
  $('mode').textContent = 'LIVE · Rust / WASM · browser compatibility path (unexecuted)';
  $('actuation-profile').hidden = true; $('replay').title = 'Choose a saved drive run (JSON) to re-execute through Rust';
  $('input-help').textContent = 'Press Play, then drive with the bindings listed above: keys by physical position, standard-layout gamepads. The page sends requests only; Rust applies the profile\'s limits, mixing and deadman on simulation time. Releasing every input sends one zero request; Esc, leaving the window or hiding the page requests stop and disarms held inputs until they are released.';
  $('performance').textContent = 'Waiting for physics'; $('speed').disabled = true;
  return true;
}
// Send DriveRequest values in order; the worker queue keeps that order. Nothing
// is sent while Rust replays a recording.
function sendDrive(requests) {
  if (!drive || !worker || drive.replaying || drive.replayPending || !requests?.length) return;
  const token = epoch;
  for (const request of requests) worker.request('drive_request', { request }).then(
    next => { if (token === epoch && drive) drive.panel.status(next); },
    error => { if (token === epoch) status(`Drive request refused: ${error.message}`, true); });
}
// Pause: after the stop request (same worker queue), Rust invalidates any live
// request (DriveSession::pause), so on resume the profile's on-loss rule runs
// until a fresh request. Rust ignores it while replaying; none is sent then.
function pauseDrive() {
  if (!drive || !worker || drive.replaying || drive.replayPending) return;
  const token = epoch;
  worker.request('drive_pause', {}).then(
    next => { if (token === epoch && drive) drive.panel.status(next); },
    error => { if (token === epoch) status(`Drive pause refused: ${error.message}`, true); });
}
// Once per animation frame: poll devices through Rust while running; refresh the panel.
function driveTick(now) {
  if (!drive || !worker) return;
  try {
    if ($('leaderboard-dialog').open) sendDrive(drive.input.textFocus());
    if (playing && !drive.replaying && !drive.replayPending) {
      let pads = [];
      try { pads = gamepadSnapshot(navigator.getGamepads?.() ?? []); } catch { pads = []; }
      sendDrive(drive.input.poll(pads, now));
    }
    drive.panel.input(drive.input.state()); drive.panel.render(now);
  } catch (error) { setPlaying(false); status(`Drive input failed: ${errorText(error)}`, true); }
}
const driveReplayFile = Object.assign(document.createElement('input'), { type: 'file', accept: 'application/json,.json', hidden: true });
driveReplayFile.id = 'drive-replay-file'; driveReplayFile.setAttribute('aria-label', 'Saved drive run to replay'); document.body.append(driveReplayFile);
driveReplayFile.onchange = () => { const file = driveReplayFile.files[0]; driveReplayFile.value = ''; if (file && drive) replayDrive(file); };
// Rust checks the recording's identity against the loaded drive; a refusal is shown verbatim.
async function replayDrive(file) {
  setPlaying(false); const token = epoch; let text;
  try { text = await file.text(); } catch (e) { status(`Could not read ${file.name}: ${e.message}`, true); return; }
  if (token !== epoch || !drive || !worker) return;
  status('Checking the recording against the loaded drive…'); const replaying = drive; replaying.replayPending = true;
  $('play').disabled = $('step').disabled = $('replay').disabled = true; $('cancel').hidden = false;
  try {
    // Replay runs ~200 periods per worker chunk (progress granularity); live chunks stay small.
    const next = await worker.request('drive_replay', { recording: text, chunk_periods: DRIVE_REPLAY_CHUNK_PERIODS },
      p => status(`Replaying recorded drive requests through Rust: ${p.completed_periods} / ${p.total_periods} periods…`));
    if (token === epoch) { showFrame(next); drive.replaying = next.replaying === true; drive.panel.status(next.drive); status(next.error || '', Boolean(next.error)); }
  } catch (e) { if (token === epoch) { drive.replaying = false; status(`Replay refused: ${e.message}`, true); } }
  finally { replaying.replayPending = false; if (token === epoch) { $('play').disabled = $('step').disabled = $('replay').disabled = false; $('cancel').hidden = true; } }
}
async function loadPreset(id) {
  videoCapture.stop();
  const token = ++epoch; abort?.abort(); worker?.close(); worker = null; drive = null; abort = new AbortController();
  setPlaying(false); busy = false; replaySaved = null; simulatedWork = wallWork = 0; playback = null;
  frame = null; lastRenderedFrame = null;
  for (const id of ['play','step','reset','timeline','download','replay']) $(id).disabled = true;
  $('replay').title = '';
  $('cancel').hidden = false; status('Loading model and controller…');
  try {
    const preset = catalog.presets.find(p => p.id === id); const data = await fetchData(preset.path, abort.signal, preset.asset_sha256); if (token !== epoch) return false;
    current = { ...preset, data }; $('description').textContent = preset.description; $('readiness').textContent = preset.readiness; $('readiness').dataset.state = preset.readiness_state || 'experimental'; $('evidence').textContent = preset.evidence;
    $('mode').textContent = preset.mode !== 'recorded' ? 'LIVE · Rust / WASM' : 'RECORDED PHYSICS'; $('view-title').textContent = preset.label;
    if (preset.mode === 'drive') { if (!await loadDrive(preset, data, token)) return false; }
    else {
    buildModel(data.robot || data.scene.robot); makeInputs([]);
    if (preset.mode !== 'recorded') {
      worker = workerClient(); const result = await worker.request('load', { scene: data.scene || data, config: data.config, task: data.task, seed: data.seed ?? 0 }); if (token !== epoch) return false;
      if (result.metadata) Object.assign(current.data, result.metadata);
      makeInputs(result.inputs); showFrame(result.frame); $('timeline').max = result.metadata ? result.metadata.steps * result.metadata.step_s : data.duration_s;
      const actuation=$('actuation-profile'),families=Object.values(data.scene?.robot?.actuator_profiles?.families??{});
      actuation.hidden=data.config?.motors?.controller!=='cad_fixed_pd'||!families.length;
      if(!actuation.hidden){
        const rates=[...new Set(families.map(f=>1/f.controller.period.value))];
        actuation.textContent=`Motor feedback/PWM: ${rates.join(' / ')} Hz · native measurement profile: ${(1/(data.config.step_s*data.config.report_every)).toFixed(0)} Hz · motion policy: ${(1/data.scene.period_s).toFixed(0)} Hz. ${families.length} CAD-bound motor parameter sets; provisional calibration.`;
      }
      $('input-help').textContent = result.metadata?.environment_contract ? `Each command is held for ${result.metadata.environment_contract.period_s*1000} ms of simulation time. The selected controller and actuator profile determine the motor response. ${data.task?.walking ? 'The task scores joint tracking, body position and supported steps'+(data.task.walking.heading?', plus heading':'')+'.' : 'Scores follow the selected task; this preset has no supported-step walking objective.'} Saving and replay preserve the task and command sequence.` : result.metadata?.policy_contract ? 'Rhai reads ideal simulated joint state and sends motor targets at its declared sampling rate. Adjust the commands above; save and replay preserve when they changed. Hardware sensor bindings and walking commands are not yet available.' : preset.mode === 'embedded' ? 'The Rust servo controller executes this experiment live. Pause and reset are available; this preset does not yet declare WASD walking commands.' : 'Use the position slider while running. This fixture has no walking command; WASD locomotion is unavailable.';
      if (current.data.policy_contract?.step_reference) $('input-help').textContent+=current.data.policy_contract.step_reference.config.sequence.update_command_before_lift?' New motion requests are checked before lift-off. A stop at that point keeps the feet planted and returns the body to standing. Airborne feet complete their landing, and reversals wait for the current transfer.':' Motion requests are latched at foot-transfer boundaries. Releasing a key finishes the current transfer before standing; this provisional crawl is deliberately slow.';
      $('performance').textContent = 'Waiting for physics'; $('speed').disabled = true;
    } else {
      $('actuation-profile').hidden=true;
      playback = data.frames; showFrame(playback[0]); $('timeline').max = playback.at(-1).time_s; $('timeline').disabled = false; $('speed').disabled = false;
      $('input-help').textContent = 'Recorded physics at simulation time. Play or scrub to inspect it; WASD and hardware control are unavailable for recordings.';
      $('performance').textContent = `${(data.simulated_s/data.stepping_wall_s).toFixed(3)}× recorded`;
    }
    }
    fit(); $('play').disabled = $('reset').disabled = $('download').disabled = false; $('step').disabled=Boolean(playback); if (drive) $('replay').disabled = false; status('');
    return true;
  } catch (error) { if (token === epoch) { setPlaying(false); status(error.message, true); $('reset').disabled = false; } return false; }
  finally { if (token === epoch) $('cancel').hidden = true; }
}
async function advanceLive(single=false) {
  if (busy || (!playing && !single) || !worker) return; busy = true; const token = epoch, before = performance.now(), old = tick;
  try { let next;
    if (drive) next = await worker.request('drive_advance', { periods: drive.periods });
    else { values=nextMotionAction(current,inputs,values);next = await worker.request('step', { action: values, response_encoding: 'json' }); }
    if (token !== epoch) return; showFrame(next); if (drive) { drive.replaying = next.replaying === true; drive.panel.status(next.drive); } else hardwareSync.onFrame();
    wallWork += (performance.now()-before)/1000; simulatedWork += tick-old;
    const liveRate = (tick-liveStartSim)/((performance.now()-liveStartWall)/1000);
    $('performance').textContent = single ? `${(simulatedWork/wallWork).toFixed(2)}× processing` : `${liveRate.toFixed(2)}× live`;
    $('performance').title = `Worker and scene-update throughput: ${(simulatedWork/wallWork).toFixed(2)}×. Live rate also includes scheduling time.`;
    if (next.done || next.error) { setPlaying(false); showFrame(next); if (next.error) status(next.error, true); }
  } catch (e) { if (token === epoch) { setPlaying(false); status(e.message, true); $('execution-state').textContent = 'Experiment stopped with an error'; } }
  finally { if (token === epoch) { busy = false; scheduleLive(); } }
}
function replayAt(time) {
  if (!playback) return; let lo = 0, hi = playback.length - 1;
  while (lo < hi) { const mid = Math.ceil((lo + hi)/2); if (playback[mid].time_s <= time) lo = mid; else hi = mid-1; }
  showFrame(playback[lo]);
}
let replayClock = 0;
renderer.setAnimationLoop(now => {
  driveTick(now);
  const elapsed = Math.min((now-lastDraw)/1000, .1); lastDraw = now;
  if (playing && playback) { replayClock += elapsed * Number($('speed').value); replayAt(replayClock); if (replayClock >= playback.at(-1).time_s) setPlaying(false); }
  controls.update();
  if (arrows.visible !== $('contacts').checked) drawNeeded = true;
  const displayHz = Number($('display-rate').value);
  if (drawNeeded && (!displayHz || now-lastSubmittedAt >= 1000/displayHz-.1)) {
    scene.updateMatrixWorld(true); selectionBox?.update(); arrows.visible = $('contacts').checked;
    renderer.render(scene,camera); drawNeeded = false;
    lastSubmittedAt = now;
    const reference = frame?.policy?.step_reference?.reference;
    lastRenderedFrame = frame ? {
      frame: renderer.info.render.frame, time_s: frame.time_s, submitted_at_ms: performance.now(),
      reference_sample: reference?.sample ?? null,
      latched_twist: reference?.latched_twist?.slice() ?? null,
    } : null;
  }
});
$('fit').onclick = () => fit(); $('fit-selected').onclick = () => { if (meshes.has(selectedName)) fit(meshes.get(selectedName)); };
$('search').oninput = () => { for (const b of $('parts').children) b.hidden = !b.dataset.name.toLowerCase().includes($('search').value.toLowerCase()); };
$('play').onclick = () => { if (!playing && tick >= Number($('timeline').max)-1e-10) { if (playback) { replayAt(0); } else { status('Reset the completed run before playing again.'); return; } } replayClock = tick; setPlaying(!playing); };
$('step').onclick = async () => { setPlaying(false); if(frame?.done){status('Reset the ended episode before stepping again.');return;} $('step').disabled=true;await advanceLive(true);if(worker&&!playback)$('step').disabled=false; };
$('timeline').oninput = () => { setPlaying(false); replayAt(Number($('timeline').value)); };
$('reset').onclick = () => loadPreset($('preset').value);
$('cancel').onclick = () => { epoch++; abort?.abort(); worker?.close(); worker = null; busy = false; setPlaying(false); status('Cancelled. Choose an experiment or reset to try again.'); $('cancel').hidden = true; $('reset').disabled = false; };
$('download').onclick = async () => {
  const token = epoch, id = current.id, recorded = Boolean(playback), driving = Boolean(drive);
  try { const data = recorded ? current.data : await worker.request(driving ? 'drive_recording' : 'recording'); if (token !== epoch) return;
    if (!recorded && !driving) { replaySaved = data; $('replay').disabled = false; }
    // A drive recording is Rust's JSON text, saved unchanged.
    const url = URL.createObjectURL(new Blob([driving ? data : JSON.stringify(data)], { type: 'application/json' })); const a = document.createElement('a'); a.href = url; a.download = `${id}.json`; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  } catch (e) { if (token === epoch) status(e.message, true); }
};
$('replay').onclick = async () => { if (drive) { setPlaying(false); driveReplayFile.click(); return; }
  setPlaying(false); if (!replaySaved || !worker) return; status('Re-executing recorded inputs…'); const token = epoch;
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
  const drive=motionCommandConfig(current,inputs);if(!drive)return;
  const requests=driveMotionValues(current,inputs,driveKeys);
  drive.command_channels.forEach((name,axis)=>{
    const i=inputs.findIndex(c=>c.name===name);if(i<0)return;const c=inputs[i];
    const slider=$('inputs').querySelectorAll('input')[i];slider.setCommandValue(requests[axis]);
  });
  for(const b of $('teleop').querySelectorAll('[data-drive-key]'))b.setAttribute('aria-pressed',String(driveKeys.has(b.dataset.driveKey)));
}
window.addEventListener('keydown', e => { if ($('leaderboard-dialog').open || /INPUT|SELECT|TEXTAREA/.test(e.target.tagName)||e.target.isContentEditable) return;
  const key=e.key.toLowerCase();if('wasd'.includes(key)&&key.length===1&&motionCommandConfig(current,inputs)){e.preventDefault();driveKeys.add(key);applyDriveKeys();return;}
  if (key==='f') fit(selectedName ? meshes.get(selectedName) : model); if (e.code==='Space') {e.preventDefault(); if (!$('play').disabled) $('play').click();} });
window.addEventListener('keyup',e=>{const key=e.key.toLowerCase();if(driveKeys.delete(key)){e.preventDefault();applyDriveKeys();}});
window.addEventListener('blur',()=>{driveKeys.clear();applyDriveKeys();});
$('task-observation-details').addEventListener('toggle',()=>{if(frame)showTaskObservations(frame);});
// Drive-profile presets only (the WASD listeners above serve the other presets).
window.addEventListener('keydown', e => { if (!drive) return;
  const textTarget = isTextField(e.target) || $('leaderboard-dialog').open, textEntry = isTextEntry(e.target), chord = isChord(e);
  if (!textTarget && !chord && drive.input.isBound(e.code)) e.preventDefault();
  sendDrive(drive.input.keyDown({ code: e.code, repeat: e.repeat, chord, textTarget, textEntry }));
});
window.addEventListener('keyup', e => { if (drive) drive.input.keyUp(e.code); });
window.addEventListener('blur', () => { if (drive) sendDrive(drive.input.stop()); });
document.addEventListener('visibilitychange', () => { if (drive && document.visibilityState === 'hidden') sendDrive(drive.input.stop()); });
document.addEventListener('focusin', e => { if (drive && isTextField(e.target)) sendDrive(drive.input.textFocus()); });
window.robotViewer = {
  scene: () => current?.data?.scene,
  begin(links) { setPlaying(false); mirrorActive = true; mirrorLinks = new Set(links); for (const [n, mesh] of meshes) mesh.material.emissive.set(mirrorLinks.has(n) ? 0x1d4a7a : 0x000000); drawNeeded = true; },
  show(poses) { if (!mirrorActive) return; applyPoses(poses); drawNeeded = true; },
  fit() { if (mirrorActive) fit(); },
  end() { if (!mirrorActive) return; mirrorActive = false; mirrorLinks.clear(); for (const mesh of meshes.values()) mesh.material.emissive.set(0x000000); if (frame) applyPoses(frame.poses); selectPart(selectedName); drawNeeded = true; },
};
const hardwareSync=installHardwareSync({snapshot:()=>current&&frame&&current.mode!=='drive'?{live:!playback,source:JSON.stringify({preset:current.id,cad:current.data.scene?.robot?.source??current.data.robot?.source}),coordinates:current.data.coordinate_names,targets:frame.servo_targets_rad,time_s:frame.time_s,done:frame.done}:null,play:()=>setPlaying(true),pause:()=>setPlaying(false)});
let catalog;
try { catalog = await fetchData('catalog.json'); for (const p of catalog.presets) { const option = document.createElement('option'); option.value = p.id; option.textContent = p.label; $('preset').append(option); }
  $('preset').disabled = false; $('preset').onchange = () => loadPreset($('preset').value);
  try { await installLeaderboard({
    pause: () => setPlaying(false),
    load: async (entry, replay) => {
      $('preset').value = entry.preset_id;
      if (!await loadPreset(entry.preset_id)) return;
      if (replay) {
        replaySaved = { version: 1, kind: 'sampled_environment_recording', task: current.data.task, error: null,
          runtime: { version: 3, kind: 'embedded_session', scene: current.data.scene, config: current.data.config,
            seed: entry.load.seed, completed_steps: entry.replay.completed_steps, input_events: entry.replay.input_events } };
        $('replay').disabled = false; $('replay').click();
      } else {
        const initial = entry.replay.input_events.find(e => e.at_step === 0)?.values;
        if (initial) restoreInputs(initial);
        setPlaying(true);
      }
    }
  }); } catch (e) { $('open-leaderboard').disabled = true; $('open-leaderboard').textContent = 'Leaderboard unavailable'; $('open-leaderboard').title = e.message; }
  const requested=new URL(location.href).searchParams.get('preset');
  const initial=catalog.presets.find(p=>p.id===requested)?.id||catalog.presets[0].id;
  $('preset').value=initial;
  await loadPreset(initial);
} catch(e) {status(e.message, true);}
