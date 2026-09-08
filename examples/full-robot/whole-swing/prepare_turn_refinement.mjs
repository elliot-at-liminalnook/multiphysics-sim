import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-turn-refinement';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const previous = read(`${root}/turn-support-plan.json`), cases = [], inputs = new Set(); mkdirSync(output, {recursive: true});
for (const [oldName, step] of [['student-turn-20ms-support-24mm', .005], ['teacher-turn-5ms-support-24mm', .0025]]) {
  const old = previous.cases.find(c => c.name === oldName), config = read(old.config);
  config.step_s = step; config.steps = Math.round(24 / step); config.report_every = Math.round(.02 / step);
  const name = `${oldName}-refined`, paths = {};
  for (const [kind, value] of Object.entries({scene: read(old.scene), config, actions: read(old.actions)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); inputs.add(old[kind]);
  }
  cases.push({...old, ...paths, name, step_s: step});
}
const plan = {version: 1, cases, sources: [...inputs, `${root}/turn-support-plan.json`, `${root}/TURN-REFINEMENT-PLAN.md`, `${root}/prepare_turn_refinement.mjs`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Paired refinement of passing short steering cases; no changed physical limits or promotion without task and accuracy checks.'};
write(`${output}/plan.json`, plan); write(`${root}/turn-refinement-plan.json`, plan); console.log(output);
