import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-refinement';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const cases = [], inputs = new Set(); mkdirSync(output, {recursive: true});
for (const [label, planPath, oldName, step] of [
  ['student', `${root}/finish-plan.json`, 'finish-0.85-20ms', .01],
  ['teacher', `${root}/feedback-teacher-plan.json`, 'feedback-teacher-20ms', .01],
  ['teacher', `${root}/feedback-teacher-plan.json`, 'feedback-teacher-20ms', .0025],
]) {
  inputs.add(planPath); const old = read(planPath).cases.find(c => c.name === oldName), config = read(old.config);
  config.step_s = step; config.steps = Math.round(24 / step); config.report_every = Math.round(.02 / step);
  const name = `${label}-${step * 1000}ms`, paths = {};
  for (const [kind, value] of Object.entries({scene: read(old.scene), config, actions: read(old.actions)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); inputs.add(old[kind]);
  }
  cases.push({...old, ...paths, name, step_s: step});
}
const plan = {version: 1, cases, sources: [...inputs, `${root}/REFINEMENT-PLAN.md`, `${root}/prepare_refinement.mjs`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Three predeclared numerical refinement cases; unchanged physical model, control sampling and acceptance budgets.'};
write(`${output}/plan.json`, plan); write(`${root}/refinement-plan.json`, plan); console.log(output);
