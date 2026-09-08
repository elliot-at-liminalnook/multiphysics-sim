import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',output='runs/full-robot/learning/transition-support';
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const write=(path,value)=>writeFileSync(path,JSON.stringify(value)+'\n');
const parentPath=`${root}/fast-distillation-holdout-plan.json`,base=read(parentPath).cases[0];
const cases=[],sources=[parentPath,`${root}/TRANSITION-SUPPORT-PLAN.md`,base.scene,base.config,base.actions,base.task,import.meta.filename];mkdirSync(output,{recursive:true});
for(const offset of [-.0225,-.024]){
  const name=`teacher-forward-support-${-offset*1000}mm`,config=read(base.config);
  assert(!existsSync(`${output}/${name}.native.json`));
  const posture=config.policy.step_reference.sequence.command_postures.find(p=>p.forward_speed_m_s===.00375);
  assert(config.policy.point_feedback.markers[1].link.startsWith('+X'));
  assert(Math.abs(posture.support_offsets_m[1][0]-(-.02145))<1e-12);
  posture.support_offsets_m[1][0]=offset;
  const paths={};for(const [key,value] of Object.entries({scene:read(base.scene),config,actions:read(base.actions)})){paths[key]=`${output}/${name}.${key}.json`;write(paths[key],value);}
  sources.push(...Object.values(paths));cases.push({...base,...paths,name,forward_front_support_x_m:offset,split:'development-after-revealed-holdout-failure'});
}
const plan={version:1,cases,sources:[...new Set(sources)].map(source),scope:'Two predeclared teacher policy support offsets on the revealed 32-second transition failure. Same CAD physical model, fine timestep, controller weights, commands and acceptance gates. This episode is now development evidence, not untouched validation. Native wall time is not a browser or isolated performance qualification.'};
write(`${output}/plan.json`,plan);write(`${root}/transition-support-plan.json`,plan);console.log(output);
