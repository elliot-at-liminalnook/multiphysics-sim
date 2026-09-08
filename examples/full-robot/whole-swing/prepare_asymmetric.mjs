import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-asymmetric';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const scenePath = 'examples/full-robot/student-speed/scene.json', configPath = 'examples/full-robot/student-speed/2x-support-18-9.config.json', taskPath = 'examples/full-robot/heading-task/task.json';
const task = read(taskPath), cases = [];
mkdirSync(output, {recursive: true});
for (const speed of [.00375, .005]) for (const order of [[0, 2, 1, 3], [3, 0, 2, 1]]) for (const step of [.02, .005]) {
  const name = `asymmetric-${speed * 1000}-${order[0] === 3 ? 'rear-first' : 'original'}-${step * 1000}ms`;
  const scene = read(scenePath), config = read(configPath), sequence = config.policy.step_reference.sequence;
  const period = sequence.phase_durations_s.reduce((s, v) => s + v, 0), original = sequence.order;
  const shift = 4 * period * (speed - .0025);
  for (const posture of sequence.command_postures) posture.forward_speed_m_s *= speed / sequence.maximum_speed_m_s;
  for (const p of [sequence, ...sequence.command_postures.filter(p => p.forward_speed_m_s >= 0)]) for (let foot = 0; foot < 4; foot++) {
    p.stance_offsets_m[foot][0] += foot === 3 ? shift : -shift;
    p.support_offsets_m[foot][0] += period * (original.indexOf(foot) * .0025 - order.indexOf(foot) * speed);
  }
  sequence.order = order; sequence.maximum_speed_m_s = speed;
  sequence.whole_swing_horizontal_motion = true; sequence.swing_body_advance_fraction = .5;
  config.step_s = step; config.steps = Math.round(24 / step); config.report_every = Math.round(task.period_s / step);
  const input = scene.controller.inputs.find(c => c.name === 'command.forward_speed'); input.lower = -speed; input.upper = speed;
  const actions = Array.from({length: Math.round(24 / task.period_s)}, (_, i) => scene.controller.inputs.map(c => c.name === input.name && i * task.period_s < 16.8 ? speed : c.initial));
  const paths = {};
  for (const [kind, value] of Object.entries({scene, config, actions})) { paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); }
  cases.push({name, speed, order, stance_shift_m: shift, duration_s: 24, step_s: step, ...paths, task: taskPath, seed: 0});
}
const plan = {version: 1, cases, sources: [scenePath, configPath, taskPath, `${root}/prepare_asymmetric.mjs`, `${root}/ASYMMETRIC-PLAN.md`, 'crates/sim-domain-control/src/stepping.rs'].map(path =>
  ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Development-only asymmetric stance and rear-first order comparison, derived from measured rear-extension limits. Same CAD physics, weights, motor bounds and physical/numerical gates. No sustained or browser acceptance implied.'};
write(`${output}/plan.json`, plan); write(`${root}/asymmetric-plan.json`, plan); console.log(output);
