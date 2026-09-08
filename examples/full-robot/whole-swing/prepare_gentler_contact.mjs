import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing';
const output = process.argv[2] ?? 'runs/full-robot/learning/gentler-contact';
assert(!existsSync(output), 'refusing to overwrite an experiment');
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const scenePath = `${root}/settled-integral-candidate.scene.json`, scene = read(scenePath);
const configPath = `${root}/settled-integral-minute.config.json`, config = read(configPath);
const archivePath = `${root}/settled-integral-regression-recipes.json`, archive = read(archivePath);
for (const s of [archive.scene, archive.task]) assert.equal(source(s.path).sha256, s.sha256, s.path);
const original = archive.cases.find(c => c.name === 'minute-1.25ms');
assert.deepEqual(original.config, config);
assert.deepEqual(scene.options.floor_friction, {kind: 'regularized_coulomb', slip_speed_m_s: .001});
const channel = scene.controller.inputs.findIndex(c => c.name === 'command.forward_speed');
assert(channel >= 0);
const stride = Math.round(read(archive.task.path).period_s / config.step_s);
let held = scene.controller.inputs.map(c => c.initial), next = 0;
const actions = Array.from({length: config.steps / stride}, (_, i) => {
  if (original.input_events[next]?.at_step === i * stride) held = original.input_events[next++].values;
  return [...held];
});
assert.equal(next, original.input_events.length);
assert.equal(createHash('sha256').update(JSON.stringify(actions)).digest('hex'), original.actions_json_sha256);
const seq = config.policy.step_reference.sequence;
const nominalStride = .00375 * seq.phase_durations_s.reduce((s, x) => s + x, 0);
for (const i of [0, 3]) seq.phase_durations_s[i] *= 2;
const period = seq.phase_durations_s.reduce((s, x) => s + x, 0), speed = nominalStride / period;
seq.maximum_speed_m_s = speed;
const posture = seq.command_postures.find(p => p.forward_speed_m_s === .00375); assert(posture);
posture.forward_speed_m_s = speed;
scene.controller.inputs[channel].upper = speed;
for (const values of actions) {
  assert(values[channel] === 0 || values[channel] === .00375);
  if (values[channel]) values[channel] = speed;
}
mkdirSync(output, {recursive: true});
const shared = {config: `${output}/gentler.config.json`, actions: `${output}/actions.json`};
writeFileSync(shared.config, JSON.stringify(config) + '\n');
writeFileSync(shared.actions, JSON.stringify(actions) + '\n');
const cases = [.001, .0001, .00003].map(slip => {
  const name = `slip-${slip * 1000}mm-s`, s = structuredClone(scene);
  s.options.floor_friction.slip_speed_m_s = slip;
  const sceneFile = `${output}/${name}.scene.json`;
  writeFileSync(sceneFile, JSON.stringify(s) + '\n');
  return {name, scene: sceneFile, ...shared, task: archive.task.path, seed: original.seed,
    duration_s: config.steps * config.step_s, step_s: config.step_s,
    slip_speed_m_s: slip, forward_speed_m_s: speed, nominal_stride_m: nominalStride, split: 'development'};
});
const plan = {version: 1, cases, maximum_contact_motion_to_body_advance_ratio: .05,
  sources: [`${root}/GENTLER-CONTACT-PLAN.md`, scenePath, configPath, archivePath, archive.task.path,
    shared.config, shared.actions, ...cases.map(c => c.scene),
    'examples/interactive/analyze_floor_contact_motion.mjs', 'examples/interactive/recorded_contact_motion.mjs',
    'examples/interactive/analyze_contact_phases.mjs', 'target/release/examples/run_environment',
    'target/release/examples/evaluate_lift', import.meta.filename].map(source),
  scope: 'Three explicit contact smoothing profiles under the fixed 2x shift/return gait. Identical CAD, controller, commands and solver settings. Original task and 5% contact screen; no timestep, held-out, terrain or browser qualification.'};
writeFileSync(`${output}/plan.json`, JSON.stringify(plan, null, 2) + '\n');
if (!process.argv[2]) writeFileSync(`${root}/gentler-contact-plan.json`, JSON.stringify(plan, null, 2) + '\n');
console.log(cases.map(c => ({name: c.name, speed_mm_s: speed * 1000})));
