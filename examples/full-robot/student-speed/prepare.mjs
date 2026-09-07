// Explicit gait experiments using the maintained learned student and solver.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';

const output = process.argv[2] ?? 'runs/full-robot/learning/student-speed';
const sources = {
  scene: 'examples/full-robot/student-distillation/scene.json',
  config: 'examples/full-robot/browser-precision/guarded-short.config.json',
  task: 'examples/full-robot/heading-task/task.json',
};
const read = p => JSON.parse(readFileSync(p));
const hash = p => createHash('sha256').update(readFileSync(p)).digest('hex');
const write = (p, value) => writeFileSync(p, JSON.stringify(value) + '\n');
const experiments = [
  {name: '1.5x', scale: 1.5},
  {name: '2x', scale: 2},
  {name: '1.5x-cadence', scale: 1.5, phases_s: [.34, .36, .36, .18, .1]},
  {name: '2x-cadence', scale: 2, phases_s: [.24, .36, .36, .18, .1]},
  {name: '1.5x-cadence-refined', scale: 1.5, phases_s: [.34, .36, .36, .18, .1], step_s: .005},
  {name: '2x-cadence-refined', scale: 2, phases_s: [.24, .36, .36, .18, .1], step_s: .005},
  {name: '2x-balanced', scale: 2, phases_s: [.28, .38, .38, .24, .1]},
  {name: '2x-balanced-refined', scale: 2, phases_s: [.28, .38, .38, .24, .1], step_s: .005},
  {name: '2x-balanced-stance', scale: 2, phases_s: [.28, .38, .38, .24, .1], positive_stance_x_m: .008},
  {name: '2x-balanced-stance-refined', scale: 2, phases_s: [.28, .38, .38, .24, .1], positive_stance_x_m: .008, step_s: .005},
  ...[-.018, -.016].flatMap(front => [.009, .007].map(rear => ({
    name: `2x-support-${Math.round(-front * 1000)}-${Math.round(rear * 1000)}`,
    scale: 2, phases_s: [.28, .38, .38, .24, .1], positive_stance_x_m: .008,
    front_support_x_m: front, rear_support_x_m: rear,
  }))),
  ...[-.018, -.016].map(front => ({
    name: `2x-support-${Math.round(-front * 1000)}-9-refined`,
    scale: 2, phases_s: [.28, .38, .38, .24, .1], positive_stance_x_m: .008,
    front_support_x_m: front, rear_support_x_m: .009, step_s: .005,
  })),
  ...[.02, .005].map(step_s => ({
    name: step_s === .02 ? '2x-support-minute' : '2x-support-minute-refined',
    scale: 2, phases_s: [.28, .38, .38, .24, .1], positive_stance_x_m: .008,
    front_support_x_m: -.018, rear_support_x_m: .009, step_s, duration_s: 60, command_duration_s: 56,
  })),
];
mkdirSync(output, {recursive: true});
const cases = [];
for (const experiment of experiments) {
  const scene = read(sources.scene), config = read(sources.config);
  const duration = experiment.duration_s ?? config.steps * config.step_s, period = read(sources.task).period_s;
  if (experiment.step_s) {
    config.step_s = experiment.step_s;
    config.steps = Math.round(duration / config.step_s);
  }
  const sequence = config.policy.step_reference.sequence;
  const speed = sequence.maximum_speed_m_s * experiment.scale;
  const input = scene.controller.inputs.find(c => c.name === 'command.forward_speed');
  assert(input, 'forward command must be declared by the scene');
  input.lower = -speed; input.upper = speed;
  sequence.maximum_speed_m_s = speed;
  for (const posture of sequence.command_postures) posture.forward_speed_m_s *= experiment.scale;
  if (experiment.phases_s) sequence.phase_durations_s = experiment.phases_s;
  if (experiment.positive_stance_x_m !== undefined) {
    for (const posture of [sequence, ...sequence.command_postures.filter(p => p.forward_speed_m_s >= 0)]) {
      for (const foot of [0, 2]) posture.stance_offsets_m[foot][0] = experiment.positive_stance_x_m;
    }
  }
  if (experiment.front_support_x_m !== undefined) {
    for (const posture of [sequence, ...sequence.command_postures.filter(p => p.forward_speed_m_s >= 0)]) {
      posture.support_offsets_m[1][0] = experiment.front_support_x_m;
      posture.support_offsets_m[3][0] = experiment.rear_support_x_m;
    }
  }
  // Preserve every declared action channel, including the student's disabled
  // external residual inputs. Do not assume an older six-channel contract.
  const actions = Array.from({length: Math.round(duration / period)}, (_, i) =>
    scene.controller.inputs.map(c => c.name === input.name && i * period < (experiment.command_duration_s ?? 16.8) ? speed : c.initial));
  const paths = Object.fromEntries(Object.entries({scene, config, actions}).map(([kind, value]) => {
    const path = `${output}/${experiment.name}.${kind}.json`; write(path, value); return [kind, path];
  }));
  cases.push({...experiment, requested_speed_m_s: speed, ...paths});
}
const plan = {
  version: 1, cases, task: sources.task,
  sources: [...Object.values(sources), 'examples/full-robot/student-speed/prepare.mjs'].map(path => ({path, sha256: hash(path)})),
  scope: 'Speed, cadence and explicit posture trials of the trained heading student, including declared timestep refinements. Same CAD physics, motor/placement bounds, network weights, feature normalization, solver tolerances, and acceptance thresholds. Higher commands extrapolate beyond training. No promotion implied.',
};
write(`${output}/plan.json`, plan);
write('examples/full-robot/student-speed/plan.json', plan);
const development = ['2x-support-18-9', '2x-support-minute', '2x-support-minute-refined'];
write('examples/full-robot/student-speed/search.recipe.json', {
  version: 1, scene: `${output}/2x-support-18-9.scene.json`,
  cases: development.map(name => ({name, config: `${output}/${name}.config.json`,
    task: sources.task, actions: `${output}/${name}.actions.json`, seed: 0})),
  search: {seed: 90724, iterations: 4, perturbation: .001},
});
console.log(output);
