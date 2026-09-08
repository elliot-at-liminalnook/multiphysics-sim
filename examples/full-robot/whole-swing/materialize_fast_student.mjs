// Keep all evaluated student variants tied to the exact deterministic fit.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',definition=`${root}/fast-distillation`,fitDir='runs/full-robot/learning/fast-distillation-fit',output='runs/full-robot/learning/fast-distillation-evaluation';
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const write=(path,value)=>writeFileSync(path,JSON.stringify(value)+'\n');
for(const s of read(`${root}/fast-distillation-fit-inputs.json`).sources)assert.equal(source(s.path).sha256,s.sha256);
assert.deepEqual(read(`${fitDir}/experiment.json`),read(`${definition}/experiment.json`));
const fit=read(`${fitDir}/fit.json`),policy=read(`${fitDir}/policy.json`),validation=read(`${fitDir}/validation.json`);
assert.deepEqual(fit.policy,policy);assert.equal(fit.best_training_loss,Math.min(fit.initial_loss,...fit.training_loss));
assert.equal(validation.final_training_loss,fit.best_training_loss);
for(const name of ['policy','fit','validation'])write(`${definition}/${name}.json`,read(`${fitDir}/${name}.json`));
const config=read(`${definition}/initial.config.json`);config.policy.neural_residual=policy;write(`${definition}/config.json`,config);
const sources=[`${root}/fast-distillation-fit-inputs.json`,`${root}/fast-distillation-study.json`,`${definition}/manifest.json`,`${definition}/scene.json`,`${definition}/initial.config.json`,`${definition}/config.json`,`${definition}/initial.policy.json`,`${definition}/policy.json`,`${definition}/fit.json`,`${definition}/validation.json`,`${root}/combined-plan.json`,`${root}/combined-status.json`,`${root}/fast-distillation-holdout-plan.json`,import.meta.filename];
const combined=read(`${root}/combined-plan.json`),heldout=read(`${root}/fast-distillation-holdout-plan.json`).cases[0];
const episodes=[['initial-turn',combined.cases.find(c=>c.name==='combined-turn-1.25ms'),false],['fitted-turn',combined.cases.find(c=>c.name==='combined-turn-1.25ms'),true],['fitted-minute',combined.cases.find(c=>c.name==='combined-minute-1.25ms'),true],['fitted-heldout',heldout,true]];
mkdirSync(output,{recursive:true});const cases=[];
for(const [name,teacher,fitted] of episodes){
  assert(!existsSync(`${output}/${name}.native.json`));
  const candidate=read(teacher.config);candidate.policy.neural_residual=fitted?policy:read(`${definition}/initial.policy.json`);
  const paths={scene:`${output}/${name}.scene.json`,config:`${output}/${name}.config.json`,actions:`${output}/${name}.actions.json`};
  write(paths.scene,read(`${definition}/scene.json`));write(paths.config,candidate);write(paths.actions,read(teacher.actions));
  sources.push(teacher.config,teacher.actions,...Object.values(paths));
  cases.push({name,...paths,task:teacher.task,duration_s:teacher.duration_s,step_s:candidate.step_s,seed:0,training_split:name.includes('heldout')?'heldout':'development',teacher_episode:teacher.name});
}
const plan={version:1,cases,sources:[...new Set(sources)].map(source),
  scope:'Initial and fitted motor-residual students use the same authored CAD scene, fine physics, bounds and complete teacher command schedules. No privileged body/point corrections are read by the motor script/network; upstream planner remains privileged. The held-out teacher schedule is retained in full despite its earlier failure. Original task gates remain unchanged.'};
write(`${output}/plan.json`,plan);write(`${root}/fast-distillation-evaluation-plan.json`,plan);
write(`${definition}/observation-boundary.json`,{version:1,deployable:false,features:policy.features,base_motor_feedback:['joint reference','ideal joint angle','recorded tracking-gain command'],
  excluded_from_motor_policy:['body position/linear velocity','foot marker state','contact forces','body and point correction suggestions'],
  still_privileged:['planner ideal body/foot state and support force','ideal gravity direction/angular velocity','ideal encoder state without measured noise or latency'],cad_declared_sensors:read(`${definition}/scene.json`).robot.sensors,
  scope:'Network feature restriction is not a hardware sensor implementation. Unused teacher feedback calculations are retained for this first controlled comparison.'});
console.log({output,training_loss:fit.best_training_loss,validation});
