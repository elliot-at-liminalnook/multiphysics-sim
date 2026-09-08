import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {captureOutcome} from '../../interactive/capture_outcome.mjs';
const root = 'examples/full-robot/whole-swing', read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const planPath = `${root}/shift-duration-plan.json`, plan = read(planPath);
const statusPath = `${root}/shift-duration-status.json`, status = read(statusPath);
const integrityPath = `${root}/shift-duration-integrity.json`, integrity = read(integrityPath);
assert(status.complete && integrity.passed);
for (const r of [plan, status, integrity]) r.sources.forEach(verify);
verify(plan.baseline); const baseline = read(plan.baseline.path);
const old = read(`${root}/settled-integral-regression-status.json`).cases.find(c => c.name === 'minute-1.25ms');
const references = r => {
  const found = new Map();
  for (const f of r.frames) {
    const p = f.policy?.step_reference?.reference;
    if (p?.phase === 'settle' && !found.has(p.step)) found.set(p.step, p);
  }
  return found;
};
const originalReferences = references(baseline);
const cases = [{...old, name: 'reference', factor: 1}, ...status.cases.map((c, i) => {
  assert.equal(c.name, plan.cases[i].name); return {...c, factor: plan.cases[i].shift_return_time_factor};
})].map(c => {
  c.sources.forEach(verify);
  const capture = c.sources.find(s => s.path.endsWith('.native.json')), r = read(capture.path);
  const config = structuredClone(r.recording.config), controller = structuredClone(r.recording.scene.controller);
  assert.deepEqual(r.recording.scene.robot, baseline.recording.scene.robot);
  assert.deepEqual(r.recording.scene.options, baseline.recording.scene.options);
  const seq = config.policy.step_reference.sequence, prior = baseline.recording.config.policy.step_reference.sequence;
  if (c.factor !== 1) {
    const planned = plan.cases.find(p => p.name === c.name);
    assert.equal(seq.maximum_speed_m_s, planned.forward_speed_m_s);
    assert.deepEqual(seq.phase_durations_s, prior.phase_durations_s.map((d, i) => [0, 3].includes(i) ? d * c.factor : d));
    seq.phase_durations_s = prior.phase_durations_s;
    seq.command_postures.find(p => p.forward_speed_m_s === planned.forward_speed_m_s).forward_speed_m_s = .00375;
    seq.maximum_speed_m_s = prior.maximum_speed_m_s;
    const input = controller.inputs.find(c => c.name === 'command.forward_speed');
    assert.equal(input.upper, planned.forward_speed_m_s);
    input.upper = baseline.recording.scene.controller.inputs.find(c => c.name === input.name).upper;
  }
  assert.deepEqual(config, baseline.recording.config);
  assert.deepEqual(controller, baseline.recording.scene.controller);
  let maximumReferenceDifference = 0, endpointCount = 0;
  for (const [step, p] of references(r)) {
    const q = originalReferences.get(step); assert(q);
    assert.equal(p.foot, q.foot);
    const a = [...p.body_world_m, ...p.feet_world_m.flat(), p.yaw_rad];
    const b = [...q.body_world_m, ...q.feet_world_m.flat(), q.yaw_rad];
    maximumReferenceDifference = Math.max(maximumReferenceDifference, ...a.map((x, j) => Math.abs(x - b[j])));
    endpointCount++;
  }
  assert(maximumReferenceDifference < 1e-12, 'planned spatial step must remain unchanged');
  const outcome = captureOutcome(r), prefix = `${root}/shift-duration-${c.name}`;
  let metrics = null, contact = null, ratio = null;
  if (r.completed || r.frames.length >= 2) {
    execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs', capture.path,
      `${prefix}-metrics.json`, ...(!r.completed ? ['--accepted-prefix'] : [])], {stdio: 'pipe'});
    metrics = read(`${prefix}-metrics.json`);
  }
  if (r.completed) {
    execFileSync(process.execPath, ['examples/interactive/analyze_floor_contact_motion.mjs', capture.path, `${prefix}-contact.json`], {stdio: 'pipe'});
    execFileSync(process.execPath, ['examples/interactive/analyze_contact_phases.mjs', capture.path, `${prefix}-phases.json`, `${prefix}-contact.json`], {stdio: 'pipe'});
    contact = read(`${prefix}-contact.json`);
    if (metrics.net_horizontal_displacement_m > 0) ratio = Math.max(...contact.feet.map(f => f.integrated_load_weighted_tangential_speed_m)) / metrics.net_horizontal_displacement_m;
  }
  return {name: c.name, time_factor: c.factor, completed: r.completed, error: c.error, outcome,
    original_task_passed: c.passed, acceptance: c.acceptance,
    verified_spatial_reference_endpoints: endpointCount, maximum_spatial_reference_difference_m: maximumReferenceDifference,
    contact_motion_to_body_advance_ratio: ratio,
    anti_sliding_screen_passed: c.passed && ratio != null && ratio <= plan.maximum_contact_motion_to_body_advance_ratio,
    sustained_windows: metrics?.sustained_windows ?? [], native_compute: metrics?.native_compute ?? null,
    positive_mechanical_work_j: metrics?.positive_mechanical_work_j ?? null,
    capture, measurements: [metrics ? `${prefix}-metrics.json` : null,
      ...(contact ? [`${prefix}-contact.json`, `${prefix}-phases.json`] : [])].filter(Boolean).map(source)};
});
writeFileSync(`${root}/shift-duration-summary.json`, JSON.stringify({version: 1, cases,
  maximum_contact_motion_to_body_advance_ratio: plan.maximum_contact_motion_to_body_advance_ratio,
  sources: [planPath, statusPath, integrityPath, import.meta.filename, 'examples/interactive/analyze_walking_capture.mjs',
    'examples/interactive/analyze_floor_contact_motion.mjs', 'examples/interactive/analyze_contact_phases.mjs',
    'examples/interactive/recorded_contact_motion.mjs', 'examples/interactive/capture_outcome.mjs'].map(source),
  browser_promoted: false,
  scope: 'Frozen timing and speed ablation at unchanged planned spatial steps, controller gains and physics. Original task and prospective contact-motion screens remain separate. Accepted prefixes do not qualify a minute. No fresh held-out, terrain, timestep or browser qualification.'}, null, 2) + '\n');
console.log(JSON.stringify(cases.map(c => ({name: c.name, completed: c.completed, error: c.error,
  task: c.original_task_passed, ratio: c.contact_motion_to_body_advance_ratio, anti_sliding: c.anti_sliding_screen_passed,
  speeds: c.sustained_windows.map(w => w.measured_sustained_speed_m_s)})), null, 2));
