import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', output = 'runs/full-robot/learning/contact-smoothing';
assert(!existsSync(output), 'refusing to overwrite the contact-smoothing study');
const read = path => JSON.parse(readFileSync(path));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const scenePath = `${root}/settled-integral-candidate.scene.json`, scene = read(scenePath);
const configPath = `${root}/settled-integral-minute.config.json`, config = read(configPath);
const recipesPath = `${root}/settled-integral-regression-recipes.json`, archive = read(recipesPath);
const original = archive.cases.find(c => c.name === 'minute-1.25ms'); assert(original);
assert.deepEqual(original.config, config);
for (const s of [archive.scene, archive.task]) assert.equal(source(s.path).sha256, s.sha256, s.path);
assert.deepEqual(scene.options.floor_friction, {kind: 'regularized_coulomb', slip_speed_m_s: .001});
const stride = Math.round(read(archive.task.path).period_s / config.step_s);
let held = scene.controller.inputs.map(c => c.initial), next = 0;
const actions = Array.from({length: config.steps / stride}, (_, i) => {
  if (original.input_events[next]?.at_step === i * stride) held = original.input_events[next++].values;
  return [...held];
});
assert.equal(next, original.input_events.length);
assert.equal(createHash('sha256').update(JSON.stringify(actions)).digest('hex'), original.actions_json_sha256);
mkdirSync(output, {recursive: true});
const actionPath = `${output}/actions.json`; writeFileSync(actionPath, JSON.stringify(actions) + '\n');
const cases = [.0001, .00003].map(slip => {
  const name = slip === .0001 ? 'slip-0.1mm-s' : 'slip-0.03mm-s';
  const s = structuredClone(scene); s.options.floor_friction.slip_speed_m_s = slip;
  const path = `${output}/${name}.scene.json`; writeFileSync(path, JSON.stringify(s) + '\n');
  return {name, scene: path, config: configPath, task: archive.task.path, actions: actionPath,
    duration_s: config.steps * config.step_s, step_s: config.step_s, seed: original.seed,
    slip_speed_m_s: slip, split: 'development'};
});
const baseline = read(`${root}/settled-integral-regression-status.json`).cases.find(c => c.name === original.name);
baseline.sources.forEach(s => assert.equal(source(s.path).sha256, s.sha256, s.path));
const plan = {version: 1, cases, baseline: baseline.sources.find(s => s.path.endsWith('.native.json')),
  maximum_contact_motion_to_body_advance_ratio: .05,
  sources: [`${root}/CONTACT-SMOOTHING-PLAN.md`, scenePath, configPath, recipesPath, archive.task.path,
    `${root}/settled-integral-regression-status.json`, `${root}/settled-integral-minute-contact-motion.json`,
    'examples/interactive/analyze_floor_contact_motion.mjs', actionPath, ...cases.map(c => c.scene),
    'target/release/examples/run_environment', 'target/release/examples/evaluate_lift', import.meta.filename].map(source),
  scope: 'Two explicit alternative contact-smoothing profiles with a frozen teacher and original development minute. Full task gates plus a prospective 5% contact-motion screen; runtime failures retained. No timestep, held-out, browser or calibrated hardware qualification.'};
for (const path of [`${output}/plan.json`, `${root}/contact-smoothing-plan.json`])
  writeFileSync(path, JSON.stringify(plan, null, 2) + '\n');
console.log(`Prepared ${cases.length} contact-smoothing profiles.`);
