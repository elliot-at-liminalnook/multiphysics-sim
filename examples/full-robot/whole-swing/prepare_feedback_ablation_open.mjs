import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',output='runs/full-robot/learning/feedback-ablation-open';
assert(!existsSync(output));mkdirSync(output,{recursive:true});
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const oldPath=`${root}/feedback-ablation-plan.json`,old=read(oldPath);old.sources.forEach(s=>assert.equal(source(s.path).sha256,s.sha256));
const scene=read(old.baseline_case.scene),changes=[];
for(const name of ['command.body_gain','command.point_gain']){
  const input=scene.controller.inputs.find(c=>c.name===name);assert(input);assert.equal(input.lower,0.25);
  changes.push({name,field:'lower',before:input.lower,after:0});input.lower=0;
}
const scenePath=`${output}/scene.json`;writeFileSync(scenePath,JSON.stringify(scene)+'\n');
const cases=[{...old.baseline_case,name:'bounds-only',disabled_moving_gain_channels:[]},...old.cases].map(c=>({...c,scene:scenePath}));
for(const c of cases)for(const a of read(c.actions)){
  assert.equal(a.length,scene.controller.inputs.length);
  a.forEach((v,i)=>{const p=scene.controller.inputs[i];assert(Number.isFinite(v)&&v>=p.lower&&v<=p.upper);});
}
const plan={version:1,cases,baseline:old.baseline,baseline_case:old.baseline_case,input_schema_changes:changes,
  maximum_contact_motion_to_body_advance_ratio:old.maximum_contact_motion_to_body_advance_ratio,
  sources:[`${root}/FEEDBACK-ABLATION-OPEN-PLAN.md`,oldPath,`${root}/feedback-ablation-status.json`,
    `${root}/feedback-ablation-integrity.json`,old.baseline_case.scene,scenePath,old.baseline_case.config,old.baseline_case.task,
    ...new Set(cases.map(c=>c.actions)),'target/release/examples/run_environment','target/release/examples/evaluate_lift',import.meta.filename].map(source),
  scope:'Explicit controller-input minima allow zero body/point gains. Bounds-only physical identity and three frozen moving-feedback ablations; physical actuator bounds, robot, all other inputs and original acceptance gates are unchanged.'};
for(const path of [`${output}/plan.json`,`${root}/feedback-ablation-open-plan.json`])writeFileSync(path,JSON.stringify(plan,null,2)+'\n');
console.log(`Prepared ${cases.length} schema-validated ablations.`);
