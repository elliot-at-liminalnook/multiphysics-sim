import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {captureOutcome} from '../../interactive/capture_outcome.mjs';
const root = 'examples/full-robot/whole-swing', read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const planPath = `${root}/contact-smoothing-plan.json`, plan = read(planPath);
const statusPath = `${root}/contact-smoothing-status.json`, status = read(statusPath);
const integrityPath = `${root}/contact-smoothing-integrity.json`, integrity = read(integrityPath);
assert(status.complete && integrity.passed);
for (const r of [plan, status, integrity]) r.sources.forEach(verify);
verify(plan.baseline); const baseline = read(plan.baseline.path);
const prior = read(`${root}/settled-integral-regression-status.json`).cases.find(c => c.name === 'minute-1.25ms');
const definitions = [{...prior, name: 'slip-1mm-s-reference', slip_speed_m_s: .001},
  ...status.cases.map((c, i) => { assert.equal(c.name, plan.cases[i].name); return {...c, slip_speed_m_s: plan.cases[i].slip_speed_m_s}; })];
const cases = definitions.map(c => {
  c.sources.forEach(verify);
  const capture = c.sources.find(s => s.path.endsWith('.native.json')), r = read(capture.path);
  assert.deepEqual(r.recording.config, baseline.recording.config);
  assert.deepEqual(r.recording.scene.robot, baseline.recording.scene.robot);
  assert.deepEqual(r.recording.scene.controller, baseline.recording.scene.controller);
  const options = structuredClone(r.recording.scene.options);
  assert.equal(options.floor_friction.slip_speed_m_s, c.slip_speed_m_s);
  options.floor_friction.slip_speed_m_s = .001;
  assert.deepEqual(options, baseline.recording.scene.options);
  // Failed episodes retain only the accepted input prefix in their recording.
  if (r.completed) assert.deepEqual(r.recording.input_events, baseline.recording.input_events);
  const outcome = captureOutcome(r), metricsPath = `${root}/contact-smoothing-${c.name}-metrics.json`;
  let metrics = null, contact = null, ratio = null;
  if (r.completed || r.frames.length >= 2) {
    execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs', capture.path,
      metricsPath, ...(!r.completed ? ['--accepted-prefix'] : [])], {stdio: 'pipe'});
    metrics = read(metricsPath);
  }
  const contactPath = `${root}/contact-smoothing-${c.name}-contact.json`;
  if (r.completed) {
    execFileSync(process.execPath, ['examples/interactive/analyze_floor_contact_motion.mjs', capture.path, contactPath], {stdio: 'pipe'});
    contact = read(contactPath);
    if (metrics.net_horizontal_displacement_m > 0) ratio = Math.max(...contact.feet.map(f =>
      f.integrated_load_weighted_tangential_speed_m)) / metrics.net_horizontal_displacement_m;
  }
  return {name: c.name, slip_speed_m_s: c.slip_speed_m_s, completed: r.completed,
    error: c.error, outcome, original_task_passed: c.passed, acceptance: c.acceptance,
    maximum_contact_motion_to_body_advance_ratio: ratio,
    prospective_anti_sliding_screen_passed: c.passed && ratio != null && ratio <= plan.maximum_contact_motion_to_body_advance_ratio,
    sustained_windows: metrics?.sustained_windows ?? [], native_compute: metrics?.native_compute ?? null,
    capture, metrics: metrics ? source(metricsPath) : null, contact: contact ? source(contactPath) : null};
});
writeFileSync(`${root}/contact-smoothing-summary.json`, JSON.stringify({version: 1, cases,
  maximum_contact_motion_to_body_advance_ratio: plan.maximum_contact_motion_to_body_advance_ratio,
  sources: [planPath, statusPath, integrityPath, import.meta.filename,
    'examples/interactive/analyze_walking_capture.mjs', 'examples/interactive/analyze_floor_contact_motion.mjs',
    'examples/interactive/capture_outcome.mjs'].map(source),
  browser_promoted: false,
  scope: 'Different explicit contact fidelity profiles with identical CAD, controller and solver settings. Original task outcomes and the prospective anti-sliding screen are separate. Full contact integrals require completed episodes; failed prefixes do not qualify the minute. No timestep convergence, terrain, new held-out or browser qualification.'}, null, 2) + '\n');
console.log(JSON.stringify(cases.map(c => ({name: c.name, completed: c.completed, error: c.error,
  task: c.original_task_passed, contact_ratio: c.maximum_contact_motion_to_body_advance_ratio,
  anti_sliding: c.prospective_anti_sliding_screen_passed})), null, 2));
