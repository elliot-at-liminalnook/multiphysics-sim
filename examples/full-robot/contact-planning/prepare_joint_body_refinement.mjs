import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning/';
const args=process.argv.slice(2);
assert(args.length===0||args.length===3,'Pass source recipe, refinement result and output prefix, or no arguments for the original experiment');
const [source,audit,prefix]=args.length?args:[d+'joint-workspace-speed.recipe.json',d+'joint-body-refinement.result.json',d+'joint-body8-speed'];
const original=JSON.parse(fs.readFileSync(source)),r=JSON.parse(fs.readFileSync(audit));
const strip=recipe=>{const x=structuredClone(recipe);delete x.candidate.motion.body;x.variables=x.variables.filter(v=>!(v.decision.kind==='motion'&&v.decision.decision.kind==='body_control'));return x;};
assert(isDeepStrictEqual(strip(original),strip(r.recipe)),'Refinement changed other decisions, bounds, model or objective');
const count=original.candidate.motion.body.keyframes.length-1,channels=original.candidate.motion.body.keyframes[0].values.length;
assert.equal(r.old_body_controls,count);assert.equal(r.new_body_controls,2*count);assert.equal(r.refined_variables,original.variables.length+count*channels);
const bodyVariables=x=>x.variables.filter(v=>v.decision.kind==='motion'&&v.decision.decision.kind==='body_control');
assert.equal(bodyVariables(original).length,count*channels);
assert.equal(bodyVariables(r.recipe).length,2*count*channels);
for(const v of bodyVariables(r.recipe)) {
  const channel=v.decision.decision.channel;
  const prior=bodyVariables(original).filter(p=>p.decision.decision.channel===channel);
  assert.equal(prior.length,count);
  assert(prior.every(p=>isDeepStrictEqual(p.bound,v.bound)),'Body-channel search bounds changed');
}
assert(r.maximum_value_error_m_or_rad<1e-12&&r.maximum_rate_error_per_s<1e-10&&r.maximum_acceleration_error_per_s2<1e-8);
const a=r.original_report.motion_report.frames,b=r.refined_report.motion_report.frames;
let matched=0,maxNormalized=0;
for(const old of a){
  const next=b.find(f=>f.clock.phase_rate===old.clock.phase_rate&&f.clock.phase_acceleration_per_s===old.clock.phase_acceleration_per_s&&Math.abs(f.time_s-old.time_s)<1e-12);assert(next,'Lost original audit frame');matched++;
  const pairs=[];
  old.wrench_residual.forEach((x,i)=>pairs.push([x,next.wrench_residual[i],i<3?original.robot.force_tolerance_n:original.robot.moment_tolerance_nm]));
  old.torque_capacity_margin_nm.forEach((x,i)=>pairs.push([x,next.torque_capacity_margin_nm[i],original.robot.torque_tolerance_nm]));
  for(const key of ['maximum_inter_link_penetration_m','maximum_floor_penetration_m'])pairs.push([old[key],next[key],original.robot.penetration_tolerance_m]);
  for(const [x,y,scale] of pairs){assert(Number.isFinite(x)&&Number.isFinite(y)&&scale>0);maxNormalized=Math.max(maxNormalized,Math.abs(x-y)/scale);}
}
assert(maxNormalized<1e-6,'Physical inequality mismatch on original frame mesh');
const output=prefix+'.recipe.json',initial=prefix+'-initial.result.json';
fs.writeFileSync(output,JSON.stringify(r.recipe,null,2)+'\n',{flag:'wx'});
fs.writeFileSync(initial,JSON.stringify(r.refined_report)+'\n',{flag:'wx'});
const identity=path=>({path,bytes:fs.statSync(path).size,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const {recipe,original_report,refined_report,...metrics}=r;
const summary={inputs:[source,audit].map(identity),outputs:[output,initial].map(identity),...metrics,matched_original_frames:matched,original_frames:a.length,refined_frames:b.length,maximum_normalized_common_frame_difference:maxNormalized,non_body_recipe_identical:true,initial_sampled_feasible:r.refined_report.sampled_feasible,scope:`Exact body spline refinement only: ${count} to ${2*count} controls, ${original.variables.length} to ${r.refined_variables} joint variables. Same body-channel bounds, other decisions, physical model, objective and recorded search budget. All original frame constraints are retained; ${b.length-a.length} additional body-knot frames are audited. No gait qualification or speed gain is claimed.`};
fs.writeFileSync(args.length?prefix+'-preparation.json':d+'joint-body-refinement-preparation.json',JSON.stringify(summary,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({matched,maxNormalized,refined_frames:b.length,variables:r.refined_variables}));
