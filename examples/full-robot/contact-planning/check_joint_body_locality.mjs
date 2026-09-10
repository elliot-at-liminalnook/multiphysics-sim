import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning/';
const read=name=>JSON.parse(fs.readFileSync(d+name));
const canonical=x=>JSON.parse(JSON.stringify(x));
const rows=[];
for(const [name,reference] of [['fast',read('joint-timed-speed-native-initial.result.json')],['warm',read('joint-timed-speed.result.json').report]]) {
 const result=read(`joint-body-locality-${name}.result.json`);
 assert(isDeepStrictEqual(canonical(result.initial_report),canonical(reference)), `${name}: full CAD baseline changed`);
 assert.equal(result.body_variables,48);assert.equal(result.groups,24);
 assert.equal(result.ordinary_two_sided_probes,96);assert.equal(result.grouped_two_sided_probes,48);
 assert.equal(result.audit_evaluations,145);assert.equal(result.frame_checks,12000);
 assert.equal(result.cases.length,48);
 rows.push({...result,initial_report:undefined,cases:undefined,baseline_full_report_identical:true});
}
const recipe=read('joint-timed-speed.recipe.json'),candidate=read('joint-timed-speed.result.json').candidate;
const body_bounds=[0,1].map(channel=>{
 const values=recipe.variables.filter(v=>v.decision.kind==='motion'&&v.decision.decision.kind==='body_control'&&v.decision.decision.channel===channel).map(v=>{
  const control=v.decision.decision.control,value=candidate.motion.body.keyframes[control].values[channel];return {control,value,bound:v.bound,normalized:(value-v.bound.lower)/(v.bound.upper-v.bound.lower)};
 });
 return {channel,values,minimum_normalized:Math.min(...values.map(v=>v.normalized)),maximum_normalized:Math.max(...values.map(v=>v.normalized))};
});
const identity=name=>{const path=d+name,bytes=fs.readFileSync(path);return {path,bytes:bytes.length,sha256:crypto.createHash('sha256').update(bytes).digest('hex')};};
const report={cases:rows,body_bounds,artifacts:['joint-body-locality-fast.result.json','joint-body-locality-warm.result.json','joint-body-locality-control-tests.log','joint-body-locality-trajectory-tests.log','check_joint_body_locality.mjs'].map(identity),scope:'Full uncached CAD comparison establishes sampled locality and grouped-probe equivalence on two recorded motions. No change to live optimizers; predicted probe reduction is not a measured speedup, new gait or physical speed bound.'};
fs.writeFileSync(d+'joint-body-locality-verification.json',JSON.stringify(report,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({cases:rows.map(x=>({...x,scope:undefined})),body_bounds:body_bounds.map(x=>({channel:x.channel,min:x.minimum_normalized,max:x.maximum_normalized}))}));
