import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-settled';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const previous = read(`${root}/sustained-plan.json`), cases = [], inputs = new Set(); mkdirSync(output, {recursive: true});
for (const old of previous.cases) {
  const scene = read(old.scene);
  scene.controller.sources = {entry: 'settled-teacher.rhai', files: {'settled-teacher.rhai': readFileSync(`${root}/settled-teacher.rhai`, 'utf8')}};
  Object.assign(scene.controller.parameters, {settled_gain_increment: .75, standing_settle_s: .4, reference_motion_tolerance_rad: 1e-8});
  const name = `settled-${old.name}`, paths = {};
  for (const [kind, value] of Object.entries({scene, config: read(old.config), actions: read(old.actions)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); inputs.add(old[kind]);
  }
  cases.push({...old, ...paths, name});
}
const plan = {version: 1, cases, sources: [...inputs, `${root}/sustained-plan.json`, `${root}/SETTLED-STANDING-PLAN.md`, `${root}/prepare_settled.mjs`, `${root}/settled-teacher.rhai`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Paired minute test of reference-history-gated standing feedback, with unchanged moving gains, motor limits and acceptance budgets.'};
write(`${output}/plan.json`, plan); write(`${root}/settled-plan.json`, plan); console.log(output);
