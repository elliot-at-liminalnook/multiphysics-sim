// Reproducible controller experiments, not accepted browser presets.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const output = process.argv[2] || 'runs/full-robot/learning/speed-envelope';
const sources = {
  scene: 'examples/full-robot/browser-terrain-contact/scene.json',
  config: 'examples/full-robot/browser-reversal/short.config.json',
  task: 'examples/full-robot/browser-reversal/task.json',
};
const read = p => JSON.parse(readFileSync(p));
const write = (p, value) => writeFileSync(p, JSON.stringify(value) + '\n');
const sha256 = p => createHash('sha256').update(readFileSync(p)).digest('hex');
const cases = [
  {name: '2x', scale: 2},
  {name: '4x', scale: 4},
  {name: '2x-posture', scale: 2, front_support_x_m: -.014},
  {name: '2x-cadence', scale: 2, phases_s: [.24, .2, .2, .26, .1]},
  {name: '2x-cadence-retry', scale: 2, phases_s: [.24, .2, .2, .26, .1], max_iterations: 80},
  {name: '1.5x-cadence', scale: 1.5, phases_s: [.34, .28, .28, .34, .1]},
  {name: '1.5x-long-swing', scale: 1.5, phases_s: [.34, .36, .36, .18, .1]},
  {name: '1.5x-stop-feedback', scale: 1.5, phases_s: [.34, .36, .36, .18, .1], standing_gain_increment: .75},
  {name: '1.5x-stop-feedback-1', scale: 1.5, phases_s: [.34, .36, .36, .18, .1], standing_gain_increment: 1},
  {name: '1.5x-stop-feedback-1-refined', scale: 1.5, phases_s: [.34, .36, .36, .18, .1], standing_gain_increment: 1, step_s: .01, max_iterations: 80},
  {name: '1.5x-refined-backtracking', scale: 1.5, phases_s: [.34, .36, .36, .18, .1], standing_gain_increment: 1, step_s: .01, max_iterations: 80, guarded_backtracking: true},
  {name: '1.5x-refined-5ms', scale: 1.5, phases_s: [.34, .36, .36, .18, .1], standing_gain_increment: 1, step_s: .005, max_iterations: 80},
  {name: '1.5x-sustained', scale: 1.5, phases_s: [.34, .36, .36, .18, .1], standing_gain_increment: 1, duration_s: 60, command_duration_s: 56},
];
mkdirSync(output, {recursive: true});
for (const experiment of cases) {
  const scene = read(sources.scene), config = read(sources.config);
  const duration = experiment.duration_s ?? config.steps * config.step_s;
  config.step_s = experiment.step_s ?? config.step_s;
  config.steps = Math.round(duration / config.step_s);
  const sequence = config.policy.step_reference.sequence;
  const speed = sequence.maximum_speed_m_s * experiment.scale;
  const channel = scene.controller.inputs.find(c => c.name === 'command.forward_speed');
  channel.lower = -speed;
  channel.upper = speed;
  sequence.maximum_speed_m_s = speed;
  for (const posture of sequence.command_postures) posture.forward_speed_m_s *= experiment.scale;
  if (experiment.phases_s) sequence.phase_durations_s = experiment.phases_s;
  if (experiment.front_support_x_m !== undefined) {
    for (const posture of [sequence, ...sequence.command_postures]) {
      posture.support_offsets_m[1][0] = experiment.front_support_x_m;
    }
  }
  if (experiment.max_iterations) config.implicit.newton.max_iterations = experiment.max_iterations;
  if (experiment.guarded_backtracking) config.implicit.newton.guarded_backtracking = true;
  if (experiment.standing_gain_increment !== undefined) scene.controller.parameters.standing_gain_increment = experiment.standing_gain_increment;
  const period = read(sources.task).period_s;
  const actions = Array.from({length: Math.round(duration / period)}, (_, i) =>
    [.5, .25, .25, i * period < (experiment.command_duration_s ?? 16.8) ? speed : 0, 0, 0]);
  for (const [kind, value] of Object.entries({scene, config, actions})) {
    write(`${output}/${experiment.name}.${kind}.json`, value);
  }
}
write(`${output}/manifest.json`, {
  version: 1, cases, sources,
  inputs: Object.fromEntries([...Object.values(sources), 'examples/full-robot/prepare_speed_envelope.mjs'].map(p => [p, sha256(p)])),
  command_duration_s: 16.8,
  scope: 'Flat-floor forward-speed experiments with ideal observations. Changes only command range, posture schedule, phase timing and explicitly listed iteration allowance or standing feedback. Physical properties, solver tolerances, actuator bounds, collision guards and acceptance budgets remain unchanged. No promotion implied.',
});
console.log(output);
