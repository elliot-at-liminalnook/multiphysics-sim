import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root='examples/full-robot/whole-swing',output=process.argv[2]??'runs/full-robot/learning/whole-velocity-seed';
const read=p=>JSON.parse(readFileSync(p)),write=(p,v)=>writeFileSync(p,JSON.stringify(v)+'\n');
mkdirSync(output,{recursive:true});
const actionPath=`${output}/tested.actions.json`;
execFileSync(process.execPath,['web/leaderboard/materialize_actions.mjs','portable-tangent-steering',actionPath]);
const cases=[];
for(const enabled of [false,true]){
  const name=`velocity-seed-${enabled?'enabled':'reference'}`,config=read(`${root}/portable-tangent-turn.config.json`),paths={};
  if(enabled)config.implicit.extrapolate_velocity_seed=true;
  for(const [kind,value] of Object.entries({scene:read(`${root}/portable-tangent-turn.scene.json`),config,actions:read(actionPath)})){
    paths[kind]=`${output}/${name}.${kind}.json`;write(paths[kind],value);
  }
  cases.push({name,...paths,task:'examples/full-robot/heading-task/task.json',seed:0,duration_s:24,step_s:.02});
}
const plan={version:1,cases,sources:[`${root}/VELOCITY-PREDICTOR-PLAN.md`,`${root}/prepare_velocity_seed.mjs`,`${root}/portable-tangent-turn.scene.json`,
  `${root}/portable-tangent-turn.config.json`,'web/leaderboard/evaluations.json','web/leaderboard/materialize_actions.mjs',
  'crates/sim-domain-robot/src/articulated/embedding/implicit.rs','crates/sim-domain-robot/tests/embedding.rs',
  'crates/sim-domain-robot/tests/embedded_step.rs','crates/sim-solve/src/lib.rs','crates/sim-solve/tests/convergence.rs'].map(path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope:'Same 24-second steering task and exact physical model/solver tolerances with guarded temporal velocity seed off/on. Seed selection requires improved exact residuals; failure retries original velocities with fresh exact derivatives.'};
write(`${output}/plan.json`,plan);write(`${root}/velocity-seed-plan.json`,plan);
