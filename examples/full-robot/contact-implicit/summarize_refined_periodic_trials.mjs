// Compare shared-Rust planning and geometry reports; no replacement physics.
import fs from 'node:fs';import assert from 'node:assert/strict';import {isDeepStrictEqual} from 'node:util';
const root='examples/full-robot/contact-implicit/',read=n=>JSON.parse(fs.readFileSync(root+n));
const rows=[];
for(const mode of ['uniform','recorded']) {
  const name='smooth16-'+mode,recipe=read(name+'.recipe.json'),prior=read('smooth8-'+mode+'.recipe.json');
  const result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json'),initial=read(name+'-initial.audit.json');
  for(const key of ['contact','actuators','position_scales','velocity_reference','velocity_scales','force_tolerance_n','moment_tolerance_nm','torque_tolerance_nm','maximum_point_penetration_m','contact_sliding_work_scale_j'])assert(isDeepStrictEqual(recipe.config[key],prior.config[key]),'changed model/objective/gate '+key);
  assert(isDeepStrictEqual(audit.planning,result.stages.at(-1).result.report),'independent audit differs');
  assert(isDeepStrictEqual(audit.geometry,dense.geometry),'same geometry times differ');
  assert.equal(audit.planning.frames.length,32);assert.equal(dense.planning.frames.length,128);
  audit.planning.frames.forEach((f,k)=>assert(isDeepStrictEqual(f,dense.planning.frames[4*k+3]),'same-time physics differs'));
  const metrics=r=>({force_error_n:r.maximum_force_error_n,moment_error_nm:r.maximum_moment_error_nm,minimum_torque_margin_nm:r.minimum_torque_margin_nm,within_planning_tolerances:r.within_planning_tolerances});
  const summary=read(name+'.summary.json').stages.at(-1);
  rows.push({mode,initial:metrics(initial.planning),coarse:metrics(audit.planning),dense:metrics(dense.planning),
    planned_displacement_rate_m_s:summary.planned_mean_diagonal_body_displacement_rate_m_s,loaded_slip_ratio:summary.maximum_planned_loaded_slip_ratio,
    maximum_interlink_overlap_m:Math.max(...audit.geometry.map(f=>f.maximum_inter_link_penetration_m)),
    maximum_floor_penetration_m:-Math.min(...audit.geometry.flatMap(f=>f.floor_clearances.map(c=>c.minimum_clearance_m))),
    solver:result.stages.at(-1).result.search});
}
console.log(JSON.stringify({scope:'Refine the motion basis without changing the initial physical curve, then optimize at the same physical times. Dense audits remain independent. No runtime performance, global speed ceiling or stable orbit claim.',controls:16,free_variables:288,base_balance_components:192,coarse_samples:32,dense_samples:128,rows},null,2));
