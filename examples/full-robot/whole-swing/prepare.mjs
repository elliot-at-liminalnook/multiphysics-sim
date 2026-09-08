import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing';
const output = process.argv[2] ?? 'runs/full-robot/learning/whole-swing';
const scenePath = 'examples/full-robot/student-speed/scene.json';
const configPath = 'examples/full-robot/student-speed/2x-support-18-9.config.json';
const taskPath = 'examples/full-robot/heading-task/task.json';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const task = read(taskPath), cases = [];
const variants = [{speed: .0025, stance: 'lateral', support: 'original'},
  ...[.00375, .005].flatMap(speed => ['lateral', 'all'].flatMap(stance =>
    ['original', 'derived'].map(support => ({speed, stance, support}))))];
mkdirSync(output, {recursive: true});
for (const variant of variants) for (const step of [.02, .005]) {
  const {speed, stance, support} = variant;
  const name = `whole-${speed * 1000}-${stance}-${support}-${step * 1000}ms`;
  const scene = read(scenePath), config = read(configPath), sequence = config.policy.step_reference.sequence;
  config.step_s = step; config.steps = Math.round(24 / step); config.report_every = Math.round(task.period_s / step);
  const period = sequence.phase_durations_s.reduce((s, v) => s + v, 0);
  const shift = period * sequence.order.length * (speed - .0025);
  for (const posture of sequence.command_postures) posture.forward_speed_m_s *= speed / sequence.maximum_speed_m_s;
  for (const posture of [sequence, ...sequence.command_postures.filter(p => p.forward_speed_m_s >= 0)]) {
    for (const foot of stance === 'all' ? [0, 1, 2, 3] : [0, 2]) posture.stance_offsets_m[foot][0] -= shift;
    if (support === 'derived') posture.support_offsets_m[1][0] -= 2 * period * (speed - .0025);
  }
  sequence.maximum_speed_m_s = speed; sequence.swing_body_advance_fraction = .5;
  sequence.whole_swing_horizontal_motion = true;
  const input = scene.controller.inputs.find(c => c.name === 'command.forward_speed');
  input.lower = -speed; input.upper = speed;
  const actions = Array.from({length: Math.round(24 / task.period_s)}, (_, i) =>
    scene.controller.inputs.map(c => c.name === input.name && i * task.period_s < 16.8 ? speed : c.initial));
  const paths = {};
  for (const [kind, value] of Object.entries({scene, config, actions})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value);
  }
  cases.push({name, ...variant, stance_shift_m: shift, duration_s: 24, step_s: step, ...paths, task: taskPath, seed: 0});
}
const plan = {version: 1, cases, sources: [scenePath, configPath, taskPath,
  `${root}/prepare.mjs`, `${root}/PLAN.md`, 'crates/sim-domain-control/src/stepping.rs'].map(path =>
  ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Development-only horizontal motion across the full swing, with geometry-derived stance/support comparisons. Same CAD physics, selected weights, motor limits and physical/numerical gates. No sustained or browser acceptance implied.'};
write(`${output}/plan.json`, plan); write(`${root}/plan.json`, plan); console.log(output);
