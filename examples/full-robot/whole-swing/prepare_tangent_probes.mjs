import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root='examples/full-robot/whole-swing', output=process.argv[2]??'runs/full-robot/learning/whole-tangent-probes';
const read=p=>JSON.parse(readFileSync(p)), write=(p,v)=>writeFileSync(p,JSON.stringify(v)+'\n');
mkdirSync(output,{recursive:true});
const actions=`${output}/tested.actions.json`;
execFileSync(process.execPath,['web/leaderboard/materialize_actions.mjs','secant-steering-short',actions]);
const cases=[];
for(const enabled of [false,true]) {
  const name=`tangent-probes-${enabled?'enabled':'reference'}`, config=read(`${root}/broyden-turn.config.json`), paths={};
  if(enabled)config.implicit.linearized_jacobian_probes=true;
  for(const [kind,value] of Object.entries({scene:read(`${root}/broyden-turn.scene.json`),config,actions:read(actions)})) {
    paths[kind]=`${output}/${name}.${kind}.json`;write(paths[kind],value);
  }
  cases.push({name,...paths,task:'examples/full-robot/heading-task/task.json',seed:0,duration_s:24,step_s:.02});
}
const plan={version:1,cases,sources:[`${root}/TANGENT-PROBE-PLAN.md`,`${root}/prepare_tangent_probes.mjs`,`${root}/broyden-turn.config.json`,
  `${root}/broyden-turn.scene.json`,'web/leaderboard/evaluations.json','web/leaderboard/materialize_actions.mjs',
  'crates/sim-domain-robot/src/articulated/embedding/implicit.rs','crates/sim-domain-robot/tests/embedding.rs',
  'crates/sim-domain-robot/tests/embedded_step.rs','crates/sim-solve/src/lib.rs'].map(path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope:'Same 20 ms steering profile, seed, controls, world, physics and acceptance with derivative-only tangent probes disabled/enabled. Ordinary residuals and accepted endpoints retain full closure; failed approximate solves restart with exact derivatives.'};
write(`${output}/plan.json`,plan);write(`${root}/tangent-probes-plan.json`,plan);
