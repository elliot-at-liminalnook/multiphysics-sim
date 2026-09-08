import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root = 'examples/full-robot/whole-swing', output = process.argv[2] ?? 'runs/full-robot/learning/whole-small-secants';
const read = p => JSON.parse(readFileSync(p)), write = (p,v) => writeFileSync(p,JSON.stringify(v)+'\n');
mkdirSync(output,{recursive:true});
const actionPath = `${output}/tested.actions.json`;
execFileSync(process.execPath,['web/leaderboard/materialize_actions.mjs','secant-steering-short',actionPath]);
const cases = [];
for (const enabled of [false,true]) {
  const name = `small-secants-${enabled ? 'enabled' : 'reference'}`, config = read(`${root}/broyden-turn.config.json`), paths = {};
  if (enabled) config.implicit.newton.broyden_negligible_updates = true;
  for (const [kind,value] of Object.entries({config,scene:read(`${root}/broyden-turn.scene.json`),actions:read(actionPath)})) {
    paths[kind] = `${output}/${name}.${kind}.json`; write(paths[kind],value);
  }
  cases.push({name,...paths,task:'examples/full-robot/heading-task/task.json',duration_s:24,step_s:.02});
}
const plan = {version:1,cases,sources:[`${root}/SMALL-SECANT-PLAN.md`,`${root}/prepare_small_secants.mjs`,`${root}/broyden-turn.scene.json`,
  `${root}/broyden-turn.config.json`,'web/leaderboard/evaluations.json','web/leaderboard/materialize_actions.mjs',
  'crates/sim-solve/src/lib.rs','crates/sim-solve/src/profile.rs','crates/sim-solve/tests/convergence.rs'].map(path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope:'Paired decreasing-small-correction secants; unchanged 20 ms steering controller and physical/convergence requirements.'};
write(`${output}/plan.json`,plan); write(`${root}/small-secants-plan.json`,plan);
