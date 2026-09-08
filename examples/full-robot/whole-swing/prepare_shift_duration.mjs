import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', output = 'runs/full-robot/learning/shift-duration';
assert(!existsSync(output));
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const scenePath = `${root}/settled-integral-candidate.scene.json`, scene = read(scenePath);
const configPath = `${root}/settled-integral-minute.config.json`, config = read(configPath);
const archivePath = `${root}/settled-integral-regression-recipes.json`, archive = read(archivePath);
for (const s of [archive.scene, archive.task]) assert.equal(source(s.path).sha256, s.sha256, s.path);
const original = archive.cases.find(c => c.name === 'minute-1.25ms'); assert.deepEqual(original.config, config);
const channel = scene.controller.inputs.findIndex(c => c.name === 'command.forward_speed'); assert(channel >= 0);
const stride = Math.round(read(archive.task.path).period_s / config.step_s);
let held = scene.controller.inputs.map(c => c.initial), next = 0;
const actions = Array.from({length: config.steps / stride}, (_, i) => {
  if (original.input_events[next]?.at_step === i * stride) held = original.input_events[next++].values;
  return [...held];
});
assert.equal(next, original.input_events.length);
assert.equal(createHash('sha256').update(JSON.stringify(actions)).digest('hex'), original.actions_json_sha256);
const sequence = config.policy.step_reference.sequence;
const nominalStride = .00375 * sequence.phase_durations_s.reduce((s, x) => s + x, 0);
mkdirSync(output, {recursive: true});
const cases = [2, 4].map(factor => {
  const c = structuredClone(config), s = structuredClone(scene), seq = c.policy.step_reference.sequence;
  for (const index of [0, 3]) seq.phase_durations_s[index] *= factor;
  const period = seq.phase_durations_s.reduce((s, x) => s + x, 0), speed = nominalStride / period;
  seq.maximum_speed_m_s = speed;
  const posture = seq.command_postures.find(p => p.forward_speed_m_s === .00375); assert(posture);
  posture.forward_speed_m_s = speed;
  s.controller.inputs[channel].upper = speed;
  assert(s.controller.inputs[channel].initial <= speed);
  const a = actions.map(values => { const v = [...values]; assert(v[channel] === 0 || v[channel] === .00375); if (v[channel]) v[channel] = speed; return v; });
  const name = `shift-return-${factor}x`;
  const paths = {scene: `${output}/${name}.scene.json`, config: `${output}/${name}.config.json`, actions: `${output}/${name}.actions.json`};
  for (const [key, value] of [['scene', s], ['config', c], ['actions', a]]) writeFileSync(paths[key], JSON.stringify(value) + '\n');
  return {name, ...paths, task: archive.task.path, seed: original.seed, duration_s: 60, step_s: c.step_s,
    shift_return_time_factor: factor, forward_speed_m_s: speed, nominal_transfer_s: period, nominal_stride_m: nominalStride};
});
const baseline = read(`${root}/settled-integral-regression-status.json`).cases.find(c => c.name === original.name);
const plan = {version: 1, cases, baseline: baseline.sources.find(s => s.path.endsWith('.native.json')),
  maximum_contact_motion_to_body_advance_ratio: .05,
  sources: [`${root}/SHIFT-DURATION-PLAN.md`, `${root}/contact-phase-reference.json`, `${root}/contact-smoothing-summary.json`,
    scenePath, configPath, archivePath, archive.task.path, ...cases.flatMap(c => [c.scene, c.config, c.actions]),
    'examples/interactive/analyze_floor_contact_motion.mjs', 'examples/interactive/recorded_contact_motion.mjs',
    'examples/interactive/analyze_contact_phases.mjs', 'target/release/examples/run_environment',
    'target/release/examples/evaluate_lift', import.meta.filename].map(source),
  scope: 'Two fixed-stride slower shift/return development gaits. Same CAD, physical equations, controller gains and fine solver. Original task gates plus the previously declared 5% contact screen; no browser, held-out, terrain or timestep qualification.'};
for (const path of [`${output}/plan.json`, `${root}/shift-duration-plan.json`]) writeFileSync(path, JSON.stringify(plan, null, 2) + '\n');
console.log(cases.map(c => ({name: c.name, speed_mm_s: c.forward_speed_m_s * 1000, stride_mm: c.nominal_stride_m * 1000})));
