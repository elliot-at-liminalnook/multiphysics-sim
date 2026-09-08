import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-broyden';
const read = p => JSON.parse(readFileSync(p)), write = (p,v) => writeFileSync(p, JSON.stringify(v)+'\n');
const base = read(`${root}/turn-support-plan.json`).cases.find(c => c.name === 'student-turn-20ms-support-24mm');
const cases = []; mkdirSync(output, {recursive: true});
for (const enabled of [false,true]) {
  const name = `student-turn-20ms-${enabled ? 'broyden' : 'reference'}`, config = read(base.config), scene = read(base.scene), paths = {};
  if (enabled) config.implicit.newton.broyden_updates = true;
  for (const [kind,value] of Object.entries({scene,config,actions:read(base.actions)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind],value);
  }
  cases.push({...base,...paths,name});
}
const plan = {version:1,cases,sources:[`${root}/BROYDEN-PLAN.md`,`${root}/prepare_broyden.mjs`,base.scene,base.config,base.actions,
  'crates/sim-solve/src/lib.rs','crates/sim-solve/src/profile.rs','crates/sim-solve/tests/convergence.rs'].map(path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope:'Identical 20 ms steering student with bounded secant updates disabled/enabled; unchanged model, controls, convergence and physical acceptance.'};
write(`${output}/plan.json`,plan);write(`${root}/broyden-plan.json`,plan);
