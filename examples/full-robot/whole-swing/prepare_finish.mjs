import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-finish';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const previous = read(`${root}/asymmetric-plan.json`), cases = [], inputs = new Set();
mkdirSync(output, {recursive: true});
for (const finish of [.75, .85]) for (const step of [.02, .005]) {
  const old = previous.cases.find(c => c.name === `asymmetric-3.75-original-${step * 1000}ms`);
  const name = `finish-${finish}-${step * 1000}ms`, config = read(old.config);
  config.policy.step_reference.sequence.horizontal_swing_finish_fraction = finish;
  const paths = {};
  for (const [kind, value] of Object.entries({scene: read(old.scene), config, actions: read(old.actions)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); inputs.add(old[kind]);
  }
  cases.push({...old, ...paths, name, horizontal_finish_fraction: finish});
}
const plan = {version: 1, cases, sources: [...inputs, `${root}/asymmetric-plan.json`, `${root}/FINISH-PLAN.md`, `${root}/prepare_finish.mjs`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Four predeclared development cases. Same physics and acceptance budgets; horizontal XY finishes before touchdown.'};
write(`${output}/plan.json`, plan); write(`${root}/finish-plan.json`, plan); console.log(output);
