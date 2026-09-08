import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-teacher-zero';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const previous = read(`${root}/finish-plan.json`), donor = 'examples/full-robot/neural-teacher/scene.json', cases = [], inputs = new Set();
mkdirSync(output, {recursive: true});
for (const step of [.02, .005]) {
  const old = previous.cases.find(c => c.name === `finish-0.85-${step * 1000}ms`);
  const name = `teacher-${step * 1000}ms`, config = read(old.config), scene = read(old.scene);
  config.policy.feedback_observations = true;
  // Preserve the task's neural-correction observation channels with exactly
  // zero output; the privileged teacher supplies the entire correction.
  const last = config.policy.neural_residual.layers.at(-1);
  last.weights = last.weights.map(row => row.map(() => 0)); last.biases = last.biases.map(() => 0);
  scene.controller.sources = read(donor).controller.sources;
  const paths = {};
  for (const [kind, value] of Object.entries({scene, config, actions: read(old.actions)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); inputs.add(old[kind]);
  }
  cases.push({...old, ...paths, name});
}
const plan = {version: 1, cases, sources: [...inputs, donor, `${root}/finish-plan.json`, `${root}/TEACHER-PLAN.md`, `${root}/prepare_teacher.mjs`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Two predeclared privileged teacher diagnosis cases; no student promotion or hardware validity implied.'};
write(`${output}/plan.json`, plan); write(`${root}/teacher-plan.json`, plan); console.log(output);
