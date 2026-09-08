// Reuse shared Rust point feedback as a privileged teacher for the faster student.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/hybrid-speed';
const output = process.argv[2] ?? 'runs/full-robot/learning/hybrid-speed';
const read = p => JSON.parse(readFileSync(p));
const write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const scenePath = 'examples/full-robot/student-distillation/scene.json';
const taskPath = 'examples/full-robot/heading-task/task.json';
const configPaths = ['examples/full-robot/student-speed/2x-support-18-9.config.json',
  'examples/full-robot/student-speed/2x-support-minute.config.json'];
const task = read(taskPath), cases = [];
mkdirSync(output, {recursive: true});
for (const gain of [0, .1, .25, .5]) {
  for (const duration of [24, 60]) {
    for (const step of [.02, .005]) {
      // Baseline's minute has already been evaluated; recheck both timesteps
      // here to associate the comparisons with the current shared runtime.
      if (gain === 0 && duration === 24) continue;
      const name = `point-${gain}-${duration}s-${step * 1000}ms`;
      const scene = read(scenePath), config = read(configPaths[duration === 24 ? 0 : 1]);
      config.step_s = step; config.steps = Math.round(duration / step);
      config.report_every = Math.round(task.period_s / step);
      if (gain > 0) {
        config.policy.feedback_observations = true;
        const files = scene.controller.sources.files;
        files['student.rhai'] = files['student.rhai'].replace(
          'commands[name] = r +',
          'commands[name] = sensors["command.point_gain"] * sensors[joint+".point_correction"] + r +');
      }
      const speed = config.policy.step_reference.sequence.maximum_speed_m_s;
      for (const input of scene.controller.inputs) {
        if (input.name === 'command.forward_speed') { input.lower = -speed; input.upper = speed; }
        if (input.name === 'command.point_gain') { input.lower = gain; input.upper = gain; input.initial = gain; }
      }
      const stop = duration === 24 ? 16.8 : 56;
      const actions = Array.from({length: Math.round(duration / task.period_s)}, (_, i) =>
        scene.controller.inputs.map(c => c.name === 'command.forward_speed' && i * task.period_s < stop ? speed : c.initial));
      const paths = {};
      for (const [kind, value] of Object.entries({scene, config, actions})) {
        paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value);
      }
      cases.push({name, gain, duration_s: duration, step_s: step, ...paths, task: taskPath, seed: 0});
    }
  }
}
const plan = {version: 1, cases,
  sources: [scenePath, taskPath, ...configPaths, `${root}/prepare.mjs`, `${root}/README.md`].map(path =>
    ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Development-only point-feedback hybrid, not a newly distilled student. Same CAD physics, selected network, motor and geometry bounds, timestep pairs and acceptance gates. Ideal world foot observations are privileged. Gain sweep and both failures and successes are retained.'};
write(`${output}/plan.json`, plan); write(`${root}/plan.json`, plan);
console.log(output);
