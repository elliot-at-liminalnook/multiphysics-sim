import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {captureOutcome} from '../../interactive/capture_outcome.mjs';
const root = 'examples/full-robot/whole-swing', read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const planPath = `${root}/gentler-contact-plan.json`, plan = read(planPath);
const statusPath = `${root}/gentler-contact-status.json`, status = read(statusPath);
const integrityPath = `${root}/gentler-contact-integrity.json`, integrity = read(integrityPath);
assert(status.complete && integrity.passed);
for (const r of [plan, status, integrity]) r.sources.forEach(verify);
const baselineSource = status.cases[0].sources.find(s => s.path.endsWith('.native.json'));
verify(baselineSource); const baseline = read(baselineSource.path);
assert(baseline.completed, 'full baseline needed for the controlled comparison');
const cases = status.cases.map((c, index) => {
  c.sources.forEach(verify);
  const planned = plan.cases[index]; assert.equal(c.name, planned.name);
  const capture = c.sources.find(s => s.path.endsWith('.native.json')), r = read(capture.path);
  assert.deepEqual(r.recording.config, baseline.recording.config);
  assert.deepEqual(r.recording.scene.robot, baseline.recording.scene.robot);
  assert.deepEqual(r.recording.scene.controller, baseline.recording.scene.controller);
  const options = structuredClone(r.recording.scene.options);
  assert.equal(options.floor_friction.slip_speed_m_s, planned.slip_speed_m_s);
  options.floor_friction.slip_speed_m_s = .001;
  assert.deepEqual(options, baseline.recording.scene.options);
  if (r.completed) assert.deepEqual(r.recording.input_events, baseline.recording.input_events);
  const prefix = `${root}/gentler-contact-${c.name}`;
  let metrics = null, contact = null, ratio = null;
  if (r.completed || r.frames.length >= 2) {
    execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs', capture.path,
      `${prefix}-metrics.json`, ...(!r.completed ? ['--accepted-prefix'] : [])], {stdio: 'pipe'});
    metrics = read(`${prefix}-metrics.json`);
  }
  if (r.completed) {
    execFileSync(process.execPath, ['examples/interactive/analyze_floor_contact_motion.mjs', capture.path,
      `${prefix}-contact.json`], {stdio: 'pipe'});
    execFileSync(process.execPath, ['examples/interactive/analyze_contact_phases.mjs', capture.path,
      `${prefix}-phases.json`, `${prefix}-contact.json`], {stdio: 'pipe'});
    contact = read(`${prefix}-contact.json`);
    if (metrics.net_horizontal_displacement_m > 0) ratio = Math.max(...contact.feet.map(f =>
      f.integrated_load_weighted_tangential_speed_m)) / metrics.net_horizontal_displacement_m;
  }
  return {name: c.name, slip_speed_m_s: planned.slip_speed_m_s, completed: r.completed,
    error: c.error, outcome: captureOutcome(r), original_task_passed: c.passed, acceptance: c.acceptance,
    maximum_contact_motion_to_body_advance_ratio: ratio,
    anti_sliding_screen_passed: c.passed && ratio != null && ratio <= plan.maximum_contact_motion_to_body_advance_ratio,
    sustained_windows: metrics?.sustained_windows ?? [], native_compute: metrics?.native_compute ?? null,
    native_timing_scope: index === 1
      ? 'Exploratory shared-host timing: a memory-heavy trajectory comparison overlapped part of this case; not an isolated throughput comparison.'
      : 'Native shared-host measurement; no browser realtime qualification.',
    positive_mechanical_work_j: metrics?.positive_mechanical_work_j ?? null,
    total_loaded_contact_motion_m: contact ? contact.feet.reduce((s, f) => s + f.integrated_load_weighted_tangential_speed_m, 0) : null,
    capture, measurements: [metrics ? `${prefix}-metrics.json` : null,
      ...(contact ? [`${prefix}-contact.json`, `${prefix}-phases.json`] : [])].filter(Boolean).map(source)};
});
writeFileSync(`${root}/gentler-contact-summary.json`, JSON.stringify({version: 1, cases,
  maximum_contact_motion_to_body_advance_ratio: plan.maximum_contact_motion_to_body_advance_ratio,
  identical_config_controller_and_completed_input_events: true,
  sources: [planPath, statusPath, integrityPath, import.meta.filename,
    'examples/interactive/analyze_walking_capture.mjs', 'examples/interactive/analyze_floor_contact_motion.mjs',
    'examples/interactive/recorded_contact_motion.mjs', 'examples/interactive/analyze_contact_phases.mjs',
    'examples/interactive/capture_outcome.mjs'].map(source),
  browser_promoted: false,
  scope: 'Explicit contact-model sensitivity under the same gentler gait. Original task and 5% contact screen retained. Failed prefixes cannot qualify the minute. No timestep, fresh held-out, terrain, browser or calibrated hardware qualification.'}, null, 2) + '\n');
console.log(JSON.stringify(cases.map(c => ({name: c.name, completed: c.completed, error: c.error,
  task: c.original_task_passed, ratio: c.maximum_contact_motion_to_body_advance_ratio,
  screen: c.anti_sliding_screen_passed, speeds: c.sustained_windows.map(w => w.measured_sustained_speed_m_s)})), null, 2));
