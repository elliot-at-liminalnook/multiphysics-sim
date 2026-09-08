import {readFileSync, writeFileSync, openSync, closeSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync, spawnSync} from 'node:child_process';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', run = 'runs/full-robot/learning/settled-stance';
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const planPath = `${root}/settled-stance-plan.json`, plan = read(planPath);
const statusPath = `${root}/settled-stance-status.json`, status = read(statusPath);
const integrityPath = `${root}/settled-stance-integrity.json`, integrity = read(integrityPath);
assert(status.complete && integrity.passed);
for (const r of [plan, status, integrity]) r.sources.forEach(verify);
verify(plan.previous_capture); const old = read(plan.previous_capture.path);
const frames = r => r.frames.map(({stepping_wall_s, ...f}) => f);
const oldFrames = frames(old), prefix = oldFrames.filter(f => f.time_s <= 26);
const cases = status.cases.map((c, i) => {
  c.sources.forEach(verify); assert.equal(c.name, plan.cases[i].name);
  const capture = c.sources.find(s => s.path.endsWith('.native.json')), r = read(capture.path);
  assert.deepEqual(r.recording.config, old.recording.config);
  assert.deepEqual(r.recording.scene.robot, old.recording.scene.robot);
  assert.deepEqual(r.recording.scene.options, old.recording.scene.options);
  assert.deepEqual(r.recording.input_events, old.recording.input_events);
  assert.deepEqual(frames(r).slice(0, prefix.length), prefix);
  if (c.name === 'reference') {
    assert.deepEqual(frames(r), oldFrames); assert.deepEqual(r.transitions, old.transitions);
  }
  const metricsPath = `${root}/settled-stance-${c.name}-metrics.json`;
  execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs', capture.path,
    metricsPath, ...(!r.completed ? ['--accepted-prefix'] : [])], {stdio: 'pipe'});
  const metrics = read(metricsPath), afterStop = r.frames.filter(f => f.policy?.time_s >= 26);
  let variation = 0, maximumJump = 0, reversals = 0, previousDelta;
  for (let i = 1; i < afterStop.length; i++) {
    const a = afterStop[i - 1].policy.targets, b = afterStop[i].policy.targets;
    const delta = Object.keys(a).map(k => b[k] - a[k]);
    variation += delta.reduce((s, d) => s + Math.abs(d), 0);
    maximumJump = Math.max(maximumJump, ...delta.map(Math.abs));
    if (previousDelta) reversals += delta.filter((d, j) => Math.abs(d) > 1e-5 && Math.abs(previousDelta[j]) > 1e-5 && d * previousDelta[j] < 0).length;
    previousDelta = delta;
  }
  return {name: c.name, completed: c.completed, task_passed: c.passed, error: c.error,
    acceptance: c.acceptance, pre_stop_frames_exact: prefix.length,
    original_full_reference_exact: c.name === 'reference' ? true : null,
    after_stop_target_variation_rad: variation, after_stop_maximum_target_jump_rad: maximumJump,
    after_stop_target_delta_reversals_above_1e_minus5_rad: reversals,
    positive_mechanical_work_j: metrics.positive_mechanical_work_j,
    maximum_loaded_marker_path_m: Math.max(...metrics.feet.map(f => f.sampled_loaded_tangential_path_m)),
    metrics: source(metricsPath), capture};
});
const diagnosis = [28, 30, 32].map(t => {
  const f = old.frames.find(f => f.time_s === t), b = f.policy.body_feedback.correction_rad, p = f.policy.point_feedback.correction_rad;
  const gains = old.metadata.coordinate_names.flatMap(name => {
    const joint = name.replace(/^joint\./, ''), o = f.policy.observations, correction = o[`${joint}.body_correction`];
    return Math.abs(correction) < 1e-8 ? [] : [(f.policy.targets[`${joint}.target`] - o[`${joint}.reference`]
      - .5 * (o[`${joint}.reference`] - o[`${joint}.angle`]) - .25 * o[`${joint}.point_correction`]) / correction];
  });
  return {time_s: t, body_error_world_m: f.policy.body_feedback.position_error_world_m,
    floor_forces_n: old.recording.scene.controller.parameters.support_force_channels.map(n => f.policy.observations[n]),
    inferred_body_gain_range: [Math.min(...gains), Math.max(...gains)],
    body_point_correction_cosine: b.reduce((s, v, i) => s + v * p[i], 0) / Math.hypot(...b) / Math.hypot(...p)};
});
const validation = [];
for (const scale of [-.1, 1.1]) {
  const scene = read(plan.cases[0].scene); scene.controller.parameters.settled_point_gain_scale = scale;
  const base = `${run}/invalid-scale-${scale}`, scenePath = `${base}.scene.json`, capturePath = `${base}.native.json`, logPath = `${base}.log`;
  assert(!existsSync(scenePath) && !existsSync(capturePath) && !existsSync(logPath));
  writeFileSync(scenePath, JSON.stringify(scene) + '\n');
  const out = openSync(capturePath, 'wx'), err = openSync(logPath, 'wx');
  const result = spawnSync('target/release/examples/run_environment', [scenePath, plan.cases[0].config,
    plan.cases[0].task, plan.cases[0].actions], {stdio: ['ignore', out, err]});
  closeSync(out); closeSync(err); assert(!result.error && result.status === 1);
  const r = read(capturePath);
  assert(!r.completed && r.error.includes('settled point gain scale must be in [0, 1]'));
  assert.equal(r.frames.length, 1); assert.equal(r.frames[0].time_s, 0);
  validation.push({scale, rejected_before_first_physics_frame: true, error: r.error,
    sources: [scenePath, capturePath, logPath].map(source)});
}
writeFileSync(`${root}/settled-stance-summary.json`, JSON.stringify({version: 1, cases, diagnosis, validation,
  promoted: false,
  sources: [planPath, statusPath, integrityPath, import.meta.filename,
    'examples/interactive/analyze_walking_capture.mjs', 'examples/interactive/capture_outcome.mjs'].map(source),
  scope: 'Four predeclared settled-controller ablations with exact prior reference and pre-stop frame identity. All original task gates retained. Command-reversal counts use consecutive target deltas greater than 1e-5 rad; they describe sampled oscillation, not a formal stability proof. Loaded marker travel is a sampled geometric proxy, not resolved contact-patch slip. Revealed development data, no held-out or browser qualification.'}, null, 2) + '\n');
console.log(cases.map(c => ({name: c.name, passed: c.task_passed, body_error_m: c.acceptance?.final_body_error_m,
  target_reversals: c.after_stop_target_delta_reversals_above_1e_minus5_rad, work_j: c.positive_mechanical_work_j})));
