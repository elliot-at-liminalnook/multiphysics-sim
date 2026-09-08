import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root='examples/full-robot/whole-swing',output=process.argv[2]??'runs/full-robot/learning/whole-tangent-radius';
const read=p=>JSON.parse(readFileSync(p)),write=(p,v)=>writeFileSync(p,JSON.stringify(v)+'\n');
mkdirSync(output,{recursive:true});
const actionPath=`${output}/tested.actions.json`;
execFileSync(process.execPath,['web/leaderboard/materialize_actions.mjs','tangent-steering-short',actionPath]);
const cases=[];
for(const radius of [1e-6,4e-6,1e-5]) {
  const name=`tangent-radius-${radius}`,config=read(`${root}/tangent-turn.config.json`),paths={};
  if(radius!==1e-6)config.implicit.linearized_probe_relative_step=radius;
  for(const [kind,value] of Object.entries({scene:read(`${root}/tangent-turn.scene.json`),config,actions:read(actionPath)})) {
    paths[kind]=`${output}/${name}.${kind}.json`;write(paths[kind],value);
  }
  cases.push({name,...paths,task:'examples/full-robot/heading-task/task.json',seed:0,duration_s:24,step_s:.02,probe_relative_step:radius});
}
const plan={version:1,cases,sources:[`${root}/TANGENT-RADIUS-PLAN.md`,`${root}/prepare_tangent_radius.mjs`,`${root}/tangent-turn.scene.json`,
  `${root}/tangent-turn.config.json`,'web/leaderboard/evaluations.json','web/leaderboard/materialize_actions.mjs',
  'crates/sim-domain-robot/src/articulated/embedding/implicit.rs','crates/sim-domain-robot/tests/embedding.rs',
  'crates/sim-domain-robot/tests/embedded_step.rs'].map(path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')})),
  scope:'Three numerical probe radii with identical physical steering task; unchanged host, physical and timestep accuracy gates.'};
write(`${output}/plan.json`,plan);write(`${root}/tangent-radius-plan.json`,plan);
