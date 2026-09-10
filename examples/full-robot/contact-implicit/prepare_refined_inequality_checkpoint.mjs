// Parameter/checkpoint migration only. Rust owns knot insertion and residuals.
import fs from 'node:fs';import assert from 'node:assert/strict';import {isDeepStrictEqual as same} from 'node:util';import {createHash} from 'node:crypto';
const [controlPath,refinedPath,parentAuditPath,refinedAuditPath,prefix]=process.argv.slice(2);
assert(prefix,'usage: control_recipe refined_recipe parent_inspection refined_inspection output_prefix');
const read=p=>JSON.parse(fs.readFileSync(p)),hash=path=>({path,sha256:createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const control=read(controlPath),recipe=read(refinedPath),parent=read(parentAuditPath),refined=read(refinedAuditPath);
const warm=control.warm_start,old=parent.inequality_inspection,next=refined.inequality_inspection;
assert(warm&&old&&next,'native inspections and parent checkpoint required');
assert(same(warm.values,old.values)&&same(warm.residuals,old.residuals),'parent native inspection does not reproduce checkpoint');
assert(same(recipe.search,control.search)&&same(recipe.slip_objective,control.slip_objective),'solver or slip policy changed');
assert(!recipe.warm_start,'refined draft must not carry old coordinates');
const oldConfig=structuredClone(control.config),newConfig=structuredClone(recipe.config);
for(const key of ['step_s','position_reference','periodic_cubic_subdivisions']){delete oldConfig[key];delete newConfig[key];}
assert(same(oldConfig,newConfig),'physical config changed beyond declared basis/grid encoding');
assert.equal(recipe.config.step_s*2,control.config.step_s);
assert.equal(recipe.config.periodic_cubic_subdivisions*2,control.config.periodic_cubic_subdivisions);
assert.equal(recipe.initial_positions.length-1,2*(control.initial_positions.length-1));
assert(same(next.bounds,recipe.bounds.flat()),'native bounds differ from refined recipe');
assert.equal(old.residuals.inequalities.length,next.residuals.inequalities.length);
assert.equal(warm.multipliers.length,next.residuals.inequalities.length);
assert.equal(parent.planning.frames.length,refined.planning.frames.length);
function delta(a,b){if(typeof a==='number'){assert(Number.isFinite(a)&&Number.isFinite(b));return Math.abs(a-b);}assert.equal(a.length,b.length);return a.reduce((m,v,j)=>Math.max(m,delta(v,b[j])),0);}
const tolerances={position:1e-12,velocity:1e-10,acceleration:1e-8,gaps_m:1e-12,
  contact_forces_world_n:1e-7,contact_velocities_world_m_s:1e-10,unactuated_wrench:1e-7,motor_torques_nm:1e-9};
const differences={};
for(const key of Object.keys(tolerances)) {
  differences[key]=Math.max(...parent.planning.frames.map((f,k)=>{const g=refined.planning.frames[k];assert.equal(f.time_s,g.time_s,'physics times changed');return delta(f[key],g[key]);}));
  assert(differences[key]<=tolerances[key],'refinement did not preserve '+key);
}
differences.objective=delta(old.residuals.objective,next.residuals.objective);
differences.inequalities=delta(old.residuals.inequalities,next.residuals.inequalities);
assert(differences.objective<1e-7&&differences.inequalities<1e-7,'normalized residual change exceeds roundoff allowance');
assert(refined.slip.groups.every(g=>g.continuous_mean_slip_upper_bound<recipe.slip_objective.target_ratio),'refined mean bound not interior');
assert.equal(Math.max(...refined.geometry.map(g=>g.maximum_inter_link_penetration_m)),0,'refined initial geometry overlaps');
recipe.warm_start={...warm,values:next.values,residuals:next.residuals};
recipe.provenance={control_recipe:hash(controlPath),refined_draft:hash(refinedPath),parent_inspection:hash(parentAuditPath),refined_inspection:hash(refinedAuditPath),
  operation:'Re-encode the exactly refined curve with native coordinates and freshly inspected native residuals. Preserve every physical-row multiplier, next penalty, preceding shifted norm and completed outer count. Check sample-time/row correspondence and roundoff-sized physical changes; native warm validation must exactly re-evaluate this new checkpoint.',
  physical_differences:differences,physical_difference_tolerances:tolerances,normalized_difference_tolerance:1e-7,
  unchanged:'Contact, actuator law, physical gates, slip objective/barrier, period, physical check times and optimizer settings. Refined numerical variable count doubles; equal inner iterations therefore require more evaluations.',
  bounds:recipe.provenance.bounds,fixed_displacement_roundoff:recipe.provenance.fixed_displacement_roundoff};
fs.writeFileSync(prefix+'.recipe.json',JSON.stringify(recipe,null,2)+'\n',{flag:'wx'});
const dense=structuredClone(recipe);dense.config.periodic_collocation_phases=null;dense.config.periodic_cubic_subdivisions=8;
fs.writeFileSync(prefix+'-dense.recipe.json',JSON.stringify(dense,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({prefix,parameters:next.values.length,physical_rows:next.residuals.inequalities.length,next_penalty:warm.next_penalty,differences}));
