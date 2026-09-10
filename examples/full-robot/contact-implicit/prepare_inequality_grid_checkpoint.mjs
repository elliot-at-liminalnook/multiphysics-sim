// Grid/checkpoint composition only; native Rust supplies all physical rows.
import fs from 'node:fs';import assert from 'node:assert/strict';import {isDeepStrictEqual as same} from 'node:util';import {createHash} from 'node:crypto';
const [controlPath,draftPath,parentAuditPath,newAuditPath,prefix]=process.argv.slice(2);
assert(prefix,'usage: control_recipe grid_draft parent_inspection new_inspection output_prefix');
const read=p=>JSON.parse(fs.readFileSync(p)),identity=path=>({path,sha256:createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const control=read(controlPath),recipe=read(draftPath),parent=read(parentAuditPath),next=read(newAuditPath),warm=control.warm_start;
const oldNative=parent.inequality_inspection,newNative=next.inequality_inspection;
assert(warm&&oldNative&&newNative&&!recipe.warm_start,'parent checkpoint and both native inspections required');
assert(!recipe.slip_objective.constrain_loaded_slip&&recipe.slip_objective.use_continuous_mean_bound&&recipe.slip_objective.loaded_slip_barrier_weight>0,'adapter requires mean-barrier protection and physical-only inequality rows');
for(const key of ['initial_positions','bounds','search','slip_objective'])assert(same(recipe[key],control[key]),'grid draft changed '+key);
const oldConfig=structuredClone(control.config),newConfig=structuredClone(recipe.config);
for(const key of ['periodic_cubic_subdivisions','periodic_collocation_phases']){delete oldConfig[key];delete newConfig[key];}
assert(same(oldConfig,newConfig),'grid draft changed the physical model or trajectory');
assert(same(oldNative.values,warm.values)&&same(oldNative.residuals,warm.residuals),'parent inspection differs from checkpoint');
assert(same(newNative.values,warm.values)&&same(newNative.bounds,oldNative.bounds),'grid changed optimizer coordinates or bounds');
const stride=recipe.slip_objective.point_groups.length+12+recipe.config.independent_coordinates.length;
assert.equal(oldNative.residuals.inequalities.length,parent.planning.frames.length*stride);
assert.equal(newNative.residuals.inequalities.length,next.planning.frames.length*stride);
assert.equal(warm.multipliers.length,oldNative.residuals.inequalities.length);
const oldTimes=new Map(parent.planning.frames.map((f,k)=>[f.time_s,k]));assert.equal(oldTimes.size,parent.planning.frames.length);
const newTimes=new Set(next.planning.frames.map(f=>f.time_s));assert.equal(newTimes.size,next.planning.frames.length);
const multipliers=Array(newNative.residuals.inequalities.length).fill(0),mapping=[],added=[];
let addedViolation=0;
next.planning.frames.forEach((frame,k)=>{
  const old=oldTimes.get(frame.time_s),rows=newNative.residuals.inequalities.slice(k*stride,(k+1)*stride);
  if(old===undefined){added.push({frame:k,time_s:frame.time_s});addedViolation=Math.max(addedViolation,...rows);return;}
  assert(same(frame,parent.planning.frames[old]),'old-time physics changed');
  assert(same(rows,oldNative.residuals.inequalities.slice(old*stride,(old+1)*stride)),'old-time inequality rows changed');
  for(let j=0;j<stride;j++)multipliers[k*stride+j]=warm.multipliers[old*stride+j];
  mapping.push({old_frame:old,new_frame:k,time_s:frame.time_s});
});
assert.equal(mapping.length,parent.planning.frames.length,'grid removed old checks');assert(added.length>0,'grid has no added checks');
assert(next.slip.groups.every(g=>g.continuous_mean_slip_upper_bound<recipe.slip_objective.target_ratio),'new-grid mean bound not strictly interior');
const previousNorm=Math.max(warm.previous_shifted_norm,addedViolation);
recipe.warm_start={...warm,residuals:newNative.residuals,multipliers,previous_shifted_norm:previousNorm};
recipe.provenance={control_recipe:identity(controlPath),grid_draft:identity(draftPath),parent_inspection:identity(parentAuditPath),new_inspection:identity(newAuditPath),
  operation:'Insert physical check times into the unchanged analytic curve. Preserve each old-time native row and multiplier exactly; initialize only new rows to zero multipliers. Recompute the mean/barrier objective and residual vector through native inspection. Native warm validation must exactly reproduce the new checkpoint.',
  rows_per_frame:stride,old_frame_mapping:mapping,added_frames:added,added_row_initial_maximum_violation:addedViolation,
  previous_norm_transition:{before:warm.previous_shifted_norm,after:previousNorm,rule:'max(previous shifted norm, maximum positive added-row violation); added zero-multiplier rows contribute max(g,0). This explicitly extends the history measure for the changed constraint set, not an uninterrupted-identical-problem claim.'},
  preserved:'Motion, coordinates, bounds, CAD/contact/actuators, period, slip target, barrier weight, solver settings, next penalty and completed outer count. Quadrature and the physical constraint set become denser.'};
fs.writeFileSync(prefix+'.recipe.json',JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});
const dense=structuredClone(recipe);dense.config.periodic_collocation_phases=null;dense.config.periodic_cubic_subdivisions=16;
fs.writeFileSync(prefix+'-dense.recipe.json',JSON.stringify(dense,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({prefix,old_frames:mapping.length,added_frames:added.length,rows:multipliers.length,next_penalty:warm.next_penalty,previous_norm_before:warm.previous_shifted_norm,previous_norm_after:previousNorm}));
