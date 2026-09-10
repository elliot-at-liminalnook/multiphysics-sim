import fs from 'node:fs';import assert from 'node:assert/strict';import {isDeepStrictEqual as same} from 'node:util';
import {verifyInequalityTrial,verifyContinuousMeanSlipAudit} from './verify_inequality_evidence.mjs';
const root='examples/full-robot/contact-implicit/',read=n=>JSON.parse(fs.readFileSync(root+n));
const parentResult=read('mean64-inner60.result.json'),control=read('mean64-grid128.recipe.json'),recipe=read('mean64-grid256.recipe.json');
const parent=read('mean-grid-parent-inspection.audit.json'),next=read('mean-grid256-inspection.audit.json');
const oldNative=parent.inequality_inspection,newNative=next.inequality_inspection,w=control.warm_start;
assert(same(w,parentResult.result.search.continuation),'control checkpoint changed');
assert(same(control.initial_positions,parentResult.positions),'control starting curve changed');
const oldAudit=structuredClone(parent);delete oldAudit.inequality_inspection;
assert(same(oldAudit,read('mean64-inner60.audit.json')),'parent native audit changed');
assert(same(w.values,oldNative.values)&&same(w.residuals,oldNative.residuals),'native parent checkpoint mismatch');
for(const key of ['initial_positions','bounds','search','slip_objective'])assert(same(control[key],recipe[key]),'grid comparison changed '+key);
const oldConfig=structuredClone(control.config),newConfig=structuredClone(recipe.config);delete oldConfig.periodic_cubic_subdivisions;delete newConfig.periodic_cubic_subdivisions;
assert(same(oldConfig,newConfig),'grid changed physical properties');assert.equal(recipe.config.periodic_cubic_subdivisions,2*control.config.periodic_cubic_subdivisions);
assert(same(recipe.warm_start.values,w.values)&&same(recipe.warm_start.residuals,newNative.residuals),'new checkpoint not from native inspection');
assert(same(newNative.bounds,oldNative.bounds),'native bounds changed');
const stride=recipe.provenance.rows_per_frame;
assert.equal(stride,recipe.slip_objective.point_groups.length+12+recipe.config.independent_coordinates.length);
const oldTimes=new Map(parent.planning.frames.map((f,k)=>[f.time_s,k]));let mapped=0,added=0,addedViolation=0;
next.planning.frames.forEach((f,k)=>{
  const old=oldTimes.get(f.time_s),rows=newNative.residuals.inequalities.slice(k*stride,(k+1)*stride),multipliers=recipe.warm_start.multipliers.slice(k*stride,(k+1)*stride);
  if(old===undefined){added++;assert(multipliers.every(v=>v===0),'new row multiplier is not zero');addedViolation=Math.max(addedViolation,...rows);}
  else {mapped++;assert(same(f,parent.planning.frames[old]),'old-time physics changed');assert(same(rows,oldNative.residuals.inequalities.slice(old*stride,(old+1)*stride)),'old-time rows changed');assert(same(multipliers,w.multipliers.slice(old*stride,(old+1)*stride)),'old-time multipliers changed');}
});
assert.equal(mapped,parent.planning.frames.length);assert.equal(added,recipe.provenance.added_frames.length);
assert.equal(addedViolation,recipe.provenance.added_row_initial_maximum_violation);
assert.equal(recipe.warm_start.previous_shifted_norm,Math.max(w.previous_shifted_norm,addedViolation));
for(const key of ['next_penalty','completed_outer_iterations'])assert.equal(recipe.warm_start[key],w[key]);
verifyContinuousMeanSlipAudit(recipe,next);
const initialDense=read('mean-grid-initial-dense.audit.json');verifyContinuousMeanSlipAudit(recipe,initialDense);
const byTime=new Map(initialDense.planning.frames.map(f=>[f.time_s,f]));next.planning.frames.forEach(f=>assert(same(f,byTime.get(f.time_s)),'initial dense shared-time physics changed'));
const trials=[];
let metric;
for(const name of ['mean64-grid128','mean64-grid256']) {
  const r=read(name+'.recipe.json'),result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  const v=verifyInequalityTrial(r,result,audit,dense),s=v.search;
  [audit,dense].forEach(a=>verifyContinuousMeanSlipAudit(r,a));
  metric=a=>({...v.metrics(a),maximum_continuous_mean_slip_bound:Math.max(...a.slip.groups.map(g=>g.continuous_mean_slip_upper_bound)),body_path_to_displacement_ratio:a.slip.body_path_m/a.slip.displacement_m});
  const bounds=r.bounds.flat(),active=[];let distance=Infinity;
  s.values.forEach((value,j)=>{const b=bounds[j],width=b.upper-b.lower;if(width>0){const fraction=(value-b.lower)/width,d=Math.min(fraction,1-fraction);distance=Math.min(distance,d);if(d<1e-8)active.push({parameter:j,value,lower:b.lower,upper:b.upper});}});
  trials.push({name,coarse:metric(audit),dense:metric(dense),dense_slip:dense.slip,
    planned_projected_45deg_rate_m_s:v.displacement.reduce((sum,x)=>sum+x,0)/Math.sqrt(2)/dense.planning.frames.at(-1).time_s,
    numerical_bounds:{active_free_bounds:active,minimum_distance_fraction:distance},
    search:{termination:s.termination,evaluations:s.evaluations,maximum_violation:s.maximum_violation,within_constraint_tolerance:s.within_constraint_tolerance,
      history:s.history.map(({inner,...h})=>({...h,inner_termination:inner.termination,inner_evaluations:inner.evaluations,rejected_evaluations:inner.rejected_evaluations,last_inner:inner.history.at(-1)})),next_penalty:s.continuation?.next_penalty??null}});
}
console.log(JSON.stringify({scope:'Matched physical-grid refinement on the same 64-control curve with native row/multiplier mapping and continuous mean-slip protection. Fine audits use 1024 times. No global optimality, physical speed limit, runtime qualification or speed gain is implied.',
  parent_native_audit_exact:true,old_frames_preserved:mapped,new_frames_added:added,
  previous_norm_transition:recipe.provenance.previous_norm_transition,initial_128:metric(parent),initial_256:metric(next),initial_1024:metric(initialDense),trials},null,2));
