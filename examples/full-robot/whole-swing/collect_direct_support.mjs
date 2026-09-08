import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {captureOutcome} from '../../interactive/capture_outcome.mjs';
const root = 'examples/full-robot/whole-swing', read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const planPath = `${root}/direct-support-plan.json`, plan = read(planPath);
const statusPath = `${root}/direct-support-status.json`, status = read(statusPath);
const integrityPath = `${root}/direct-support-integrity.json`, integrity = read(integrityPath);
assert(status.complete && integrity.passed);
for (const r of [plan, status, integrity]) r.sources.forEach(verify);
const frames = r => r.frames.map(({stepping_wall_s, ...f}) => f);
const landings = r => {
  const result = new Map();
  for (const f of r.frames) {
    const p = f.policy?.step_reference?.reference;
    if (p?.phase === 'return' && !result.has(p.step)) result.set(p.step, p);
  }
  return result;
};
const cases = status.cases.map((c, i) => {
  const def = plan.cases[i]; assert.equal(c.name, def.name);
  c.sources.forEach(verify); verify(def.baseline);
  const baseline = read(def.baseline.path), capture = c.sources.find(s => s.path.endsWith('.native.json')), r = read(capture.path);
  assert.deepEqual(r.recording.scene, baseline.recording.scene);
  const config = structuredClone(r.recording.config);
  if (def.direct_support_transfer) {
    assert.equal(config.policy.step_reference.sequence.direct_support_transfer, true);
    delete config.policy.step_reference.sequence.direct_support_transfer;
    assert.deepEqual(config.policy.step_reference.sequence.phase_durations_s, [.58, .38, .38, .02, .02]);
    config.policy.step_reference.sequence.phase_durations_s = baseline.recording.config.policy.step_reference.sequence.phase_durations_s;
  }
  assert.deepEqual(config, baseline.recording.config);
  if (!def.direct_support_transfer) {
    assert(r.completed);
    assert.deepEqual(frames(r), frames(baseline)); assert.deepEqual(r.transitions, baseline.transitions);
    assert.deepEqual(r.recording, baseline.recording); assert.deepEqual(r.contract, baseline.contract);
  }
  const priorLandings = landings(baseline); let landingDifference = 0, verifiedLandings = 0;
  // Turning inputs can latch at different points in a changed trajectory; only
  // constant-forward development has the same planned spatial landing sequence.
  if (c.name === 'direct-minute') for (const [step, p] of landings(r)) {
    const previous = priorLandings.get(step); assert(previous); assert.equal(p.foot, previous.foot);
    const a = p.feet_world_m.flat(), b = previous.feet_world_m.flat();
    landingDifference = Math.max(landingDifference, ...a.map((v, j) => Math.abs(v - b[j]))); verifiedLandings++;
  }
  assert(landingDifference < 1e-12, 'constant-command foot landing geometry changed');
  const prefix = `${root}/direct-support-${c.name}`, outcome = captureOutcome(r);
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
  const refs = r.frames.map(f => f.policy).filter(p => p?.step_reference);
  let maxSpeed = 0, maxAcceleration = 0, previousVelocity;
  for (let j = 1; j < refs.length; j++) {
    const a = refs[j - 1], b = refs[j], dt = b.time_s - a.time_s;
    assert(dt > 0);
    const v = b.step_reference.reference.body_world_m.map((x, k) => (x - a.step_reference.reference.body_world_m[k]) / dt);
    maxSpeed = Math.max(maxSpeed, Math.hypot(...v));
    if (previousVelocity) maxAcceleration = Math.max(maxAcceleration, Math.hypot(...v.map((x, k) => (x - previousVelocity[k]) / dt)));
    previousVelocity = v;
  }
  const isMinute = def.duration_s === 60;
  return {name: c.name, completed: r.completed, error: c.error, outcome, task_passed: c.passed, acceptance: c.acceptance,
    default_frames_transitions_recording_exact: !def.direct_support_transfer ? true : null,
    verified_forward_landings: verifiedLandings, maximum_forward_landing_difference_m: landingDifference,
    contact_motion_to_body_advance_ratio: ratio,
    minute_anti_sliding_screen_passed: isMinute ? c.passed && ratio != null && ratio <= plan.maximum_contact_motion_to_body_advance_ratio : null,
    maximum_sampled_reference_speed_m_s: maxSpeed, maximum_sampled_reference_acceleration_m_s2: maxAcceleration,
    sustained_windows: metrics?.sustained_windows ?? [], native_compute: metrics?.native_compute ?? null,
    positive_mechanical_work_j: metrics?.positive_mechanical_work_j ?? null,
    capture, measurements: [metrics ? `${prefix}-metrics.json` : null,
      ...(contact ? [`${prefix}-contact.json`, `${prefix}-phases.json`] : [])].filter(Boolean).map(source)};
});
writeFileSync(`${root}/direct-support-summary.json`, JSON.stringify({version: 1, cases,
  maximum_contact_motion_to_body_advance_ratio: plan.maximum_contact_motion_to_body_advance_ratio,
  sources: [planPath, statusPath, integrityPath, import.meta.filename, 'examples/interactive/analyze_walking_capture.mjs',
    'examples/interactive/analyze_floor_contact_motion.mjs', 'examples/interactive/analyze_contact_phases.mjs',
    'examples/interactive/recorded_contact_motion.mjs', 'examples/interactive/capture_outcome.mjs'].map(source),
  browser_promoted: false,
  scope: 'Default full identity and explicit direct body-transfer path. Planned forward foot landings stay unchanged; body reference path and phase allocation change. Sampled reference acceleration is not actual body acceleration. Original task and prospective minute contact screen are separate, with failed prefixes retained. No browser, timestep, fresh held-out or terrain qualification.'}, null, 2) + '\n');
console.log(JSON.stringify(cases.map(c => ({name: c.name, completed: c.completed, error: c.error,
  task: c.task_passed, ratio: c.contact_motion_to_body_advance_ratio, anti_sliding: c.minute_anti_sliding_screen_passed,
  speeds: c.sustained_windows.map(w => w.measured_sustained_speed_m_s)})), null, 2));
