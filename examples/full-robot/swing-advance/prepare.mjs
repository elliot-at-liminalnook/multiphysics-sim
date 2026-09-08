import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/swing-advance';
const output = process.argv[2] ?? 'runs/full-robot/learning/swing-advance';
const scenePath = 'examples/full-robot/student-distillation/scene.json';
const configPath = 'examples/full-robot/student-speed/2x-support-18-9.config.json';
const taskPath = 'examples/full-robot/heading-task/task.json';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const task = read(taskPath), cases = [];
mkdirSync(output, {recursive: true});
for (const speed of [.0025, .00375, .005]) for (const fraction of [0, .5, 1]) for (const step of [.02, .005]) {
  const name = `speed-${speed * 1000}-overlap-${fraction}-${step * 1000}ms`;
  const scene = read(scenePath), config = read(configPath);
  config.step_s = step; config.steps = Math.round(24 / step); config.report_every = Math.round(task.period_s / step);
  const sequence = config.policy.step_reference.sequence;
  for (const posture of sequence.command_postures) posture.forward_speed_m_s *= speed / sequence.maximum_speed_m_s;
  sequence.maximum_speed_m_s = speed; sequence.swing_body_advance_fraction = fraction;
  const input = scene.controller.inputs.find(c => c.name === 'command.forward_speed');
  input.lower = -speed; input.upper = speed;
  const actions = Array.from({length: Math.round(24 / task.period_s)}, (_, i) =>
    scene.controller.inputs.map(c => c.name === input.name && i * task.period_s < 16.8 ? speed : c.initial));
  const paths = {};
  for (const [kind, value] of Object.entries({scene, config, actions})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value);
  }
  cases.push({name, requested_speed_m_s: speed, swing_body_advance_fraction: fraction,
    duration_s: 24, step_s: step, ...paths, task: taskPath, seed: 0});
}
const plan = {version: 1, cases,
  sources: [scenePath, configPath, taskPath, `${root}/prepare.mjs`, `${root}/README.md`,
    'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Development-only overlap of body advance and swing using shared Rust references. Unchanged CAD physics, weights, motor limits and physical/numerical gates. All failures retained; no browser or sustained acceptance implied.'};
write(`${output}/plan.json`, plan); write(`${root}/plan.json`, plan); console.log(output);
