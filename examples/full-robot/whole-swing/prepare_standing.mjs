import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-standing';
const read = p => JSON.parse(readFileSync(p)), write = (p, v) => writeFileSync(p, JSON.stringify(v) + '\n');
const previous = read(`${root}/sustained-plan.json`), cases = [], inputs = new Set(); mkdirSync(output, {recursive: true});
for (const old of previous.cases) {
  const scene = read(old.scene); scene.controller.parameters.standing_gain_increment = 1.25;
  const name = `standing-${old.name}`, paths = {};
  for (const [kind, value] of Object.entries({scene, config: read(old.config), actions: read(old.actions)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind], value); inputs.add(old[kind]);
  }
  cases.push({...old, ...paths, name});
}
const plan = {version: 1, cases, sources: [...inputs, `${root}/sustained-plan.json`, `${root}/STANDING-PLAN.md`, `${root}/prepare_standing.mjs`,
  'crates/sim-domain-control/src/stepping.rs'].map(path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope: 'Single paired standing-gain test following minute endpoint failures; unchanged moving controller, physical model and gates.'};
write(`${output}/plan.json`, plan); write(`${root}/standing-plan.json`, plan); console.log(output);
