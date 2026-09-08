import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-sustained';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const cases = [], inputs = new Set(); mkdirSync(output, {recursive: true});
for (const [planPath, oldName] of [[`${root}/feedback-teacher-plan.json`, 'feedback-teacher-5ms'], [`${root}/refinement-plan.json`, 'teacher-2.5ms']]) {
  inputs.add(planPath); const old = read(planPath).cases.find(c => c.name === oldName), config = read(old.config), scene = read(old.scene);
  config.steps = Math.round(60 / config.step_s);
  const name = `teacher-minute-${config.step_s * 1000}ms`, paths = {};
  const actions = Array.from({length: 3000}, (_, i) => scene.controller.inputs.map(c => c.name === 'command.forward_speed' && i * .02 < 56 ? .00375 : c.initial));
  for (const [kind, value] of Object.entries({scene, config, actions})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); if (kind !== 'actions') inputs.add(old[kind]);
  }
  cases.push({...old, ...paths, name, duration_s: 60});
}
const plan = {version: 1, cases, sources: [...inputs, `${root}/SUSTAINED-PLAN.md`, `${root}/prepare_sustained.mjs`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Paired one-minute 3.75 mm/s privileged teacher development cases, with unchanged physical and numerical gates.'};
write(`${output}/plan.json`, plan); write(`${root}/sustained-plan.json`, plan); console.log(output);
