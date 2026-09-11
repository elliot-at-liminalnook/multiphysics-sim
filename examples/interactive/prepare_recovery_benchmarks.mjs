// Configure initial-state recovery through the shared embedded runtime.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const [input,planPath,output]=process.argv.slice(2);
assert(output,'usage: prepare_recovery_benchmarks input-recording plan.json fresh-directory');
const bytes=fs.readFileSync(input),base=JSON.parse(bytes),plan=JSON.parse(fs.readFileSync(planPath));
assert.equal(plan.version,1);assert(plan.duration_s>0&&Number.isFinite(plan.duration_s));
const steps=plan.duration_s/base.runtime.config.step_s;
assert(Number.isSafeInteger(Math.round(steps))&&Math.abs(steps-Math.round(steps))<1e-8);
assert(steps<=base.runtime.config.steps);
const items=[];const names=new Set();
for(const c of plan.cases){
 assert(/^[a-z0-9_]+$/.test(c.name)&&!names.has(c.name));names.add(c.name);
 const r=structuredClone(base),edits=[];
 for(const [field,offset]of [['initial_base_translation_m',c.translation_offset_m],['initial_base_rotation_vector_rad',c.rotation_vector_offset_rad]]){
  const original=r.runtime.config[field];assert(Array.isArray(original)&&original.length===3&&Array.isArray(offset)&&offset.length===3);
  assert(original.concat(offset).every(Number.isFinite));const value=original.map((v,i)=>v+offset[i]);
  edits.push({pointer:'/runtime/config/'+field,original,value});r.runtime.config[field]=value;
 }
 for(const [object,key,value,pointer]of [[r.runtime.config,'steps',Math.round(steps),'/runtime/config/steps'],[r.runtime,'completed_steps',Math.round(steps),'/runtime/completed_steps'],[r.runtime.scene,'duration_s',plan.duration_s,'/runtime/scene/duration_s']]){
  edits.push({pointer,original:object[key],value});object[key]=value;
 }
 r.runtime.input_events=r.runtime.input_events.filter(e=>e.at_step<steps);
 const content=JSON.stringify(r)+'\n';items.push({name:c.name,content,edits,sha256:createHash('sha256').update(content).digest('hex')});
}
fs.mkdirSync(output);
for(const i of items)fs.writeFileSync(`${output}/${i.name}.input.json`,i.content,{flag:'wx'});
fs.writeFileSync(output+'/manifest.json',JSON.stringify({version:1,source:input,source_sha256:createHash('sha256').update(bytes).digest('hex'),plan:planPath,
 cases:items.map(({content,...i})=>i),scope:plan.scope,action_change:'Retain original scheduled command values/times within the shorter evaluation horizon.'},null,2)+'\n',{flag:'wx'});
