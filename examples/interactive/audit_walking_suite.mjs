// Bind all evaluation outcomes to their authored recipes and captured physics.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {captureOutcome} from './capture_outcome.mjs';
const [planPath, statusPath, output] = process.argv.slice(2);
const auditOptions = process.argv.slice(5);
assert(auditOptions.length <= 1 && auditOptions.every(a => /^--seed=\d+$/.test(a)), 'expected optional --seed=N');
const seedArgument = auditOptions[0];
const declaredSeedDefault = seedArgument === undefined ? undefined : Number(seedArgument.slice(7));
assert(declaredSeedDefault === undefined || Number.isSafeInteger(declaredSeedDefault) && declaredSeedDefault >= 0, 'invalid declared seed');
assert(planPath && statusPath && output, 'usage: audit_walking_suite.mjs plan.json status.json audit.json [--seed=N]');
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const plan = read(planPath), status = read(statusPath), omitted = new Set();
assert(status.complete); assert.equal(status.cases.length, plan.cases.length);
assert.equal(new Set(plan.cases.map(c => c.name)).size, plan.cases.length);
for (const s of plan.sources) assert.equal(source(s.path).sha256, s.sha256, s.path);
function authored(a, b, path, scene = false) {
  if (a && typeof a === 'object') {
    assert(b && typeof b === 'object', path);
    if (Array.isArray(a)) assert.equal(a.length, b.length, path);
    for (const key of Object.keys(a)) {
      if (!Object.hasOwn(b, key)) {
        // A known backward-compatible optional zero is omitted by Rust serde.
        if (key === 'swing_body_advance_fraction' && a[key] === 0) continue;
        assert(scene, `authored configuration field missing: ${path}.${key}`);
        omitted.add(`${path}.${key}`.replace(/\.\d+/g, '.*'));
      } else authored(a[key], b[key], `${path}.${key}`, scene);
    }
  } else assert(a === b, `recipe mismatch: ${path}`);
}
let robot, options;
const outcomes = [];
for (const [index, c] of plan.cases.entries()) {
  const result = status.cases[index]; assert.equal(result.name, c.name);
  for (const s of result.sources) assert.equal(source(s.path).sha256, s.sha256, s.path);
  const captured = result.sources.find(s => s.path.endsWith('.native.json')); assert(captured);
  const r = read(captured.path), config = read(c.config), scene = read(c.scene), actions = read(c.actions);
  authored(config, r.recording.config, 'config'); authored(scene, r.recording.scene, 'scene', true);
  assert.deepEqual(read(c.task), r.task);
  const declaredSeed = c.seed ?? declaredSeedDefault;
  assert(Number.isSafeInteger(declaredSeed), 'declare the seed in each case or pass --seed=N explicitly');
  assert.equal(r.recording.seed, declaredSeed);
  if (robot) { assert.deepEqual(r.recording.scene.robot, robot); assert.deepEqual(r.recording.scene.options, options); }
  else { robot = r.recording.scene.robot; options = r.recording.scene.options; }
  const stride = r.task.period_s / config.step_s;
  assert.equal(stride, Math.round(stride));
  let held = scene.controller.inputs.map(i => i.initial), event = 0;
  for (let i = 0; i < r.frames.length - 1; i++) {
    if (r.recording.input_events[event]?.at_step === i * stride) held = r.recording.input_events[event++].values;
    assert.deepEqual(held, actions[i], `${c.name} action ${i}`);
    assert.deepEqual(r.frames[i + 1].policy_inputs, actions[i], `${c.name} frame ${i + 1}`);
  }
  for (const e of r.recording.input_events) {
    assert.equal(e.at_step % stride, 0);
    assert.deepEqual(e.values, actions[e.at_step / stride]);
  }
  assert.equal(r.completed, result.completed); assert.equal(r.error, result.error);
  const outcome = captureOutcome(r);
  if (r.completed) {
    assert.equal(r.frames.length, actions.length + 1); assert.equal(event, r.recording.input_events.length);
    assert(result.acceptance);
    const a = read(result.acceptance.source.path);
    assert.equal(source(result.acceptance.source.path).sha256, result.acceptance.source.sha256);
    assert.equal(a.capture.sha256, captured.sha256); assert.equal(a.passed, result.passed);
    assert.deepEqual(a.budgets, result.acceptance.budgets);
  } else { assert.equal(result.passed, false); assert.equal(result.acceptance, null); }
  outcomes.push({name: c.name, completed: r.completed, passed: result.passed,
    completed_transitions: r.frames.length - 1, outcome, input_identity_verified: true, verified_seed: declaredSeed});
}
writeFileSync(output, JSON.stringify({version: 1, passed: true, outcomes,
  explicit_seed_default: declaredSeedDefault ?? null,
  identical_parsed_robot_and_physics_options: true,
  unretained_source_scene_fields: [...omitted].sort(),
  sources: [source(planPath), source(statusPath), source(import.meta.filename),
    source('examples/interactive/capture_outcome.mjs')],
  scope: 'Every declared outcome matched to hashed inputs and captures, authored configuration, all retained scene fields, seed and sampled actions; completed captures require matching independent acceptance. Raw CAD exports retain additional fields absent from the Rust scene schema, listed explicitly. This integrity audit is not a controller acceptance or hardware certificate.'}, null, 2) + '\n');
