// Compare optional Rust slip shaping against independent dense physical evidence.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const reference=read('slip32-control.recipe.json'),markers=read('surface-markers.json').markers;
const links=[...new Set(markers.map(m=>m.link))];
const metrics=a=>({samples:a.planning.frames.length,force_error_n:a.planning.maximum_force_error_n,
  moment_error_nm:a.planning.maximum_moment_error_nm,minimum_torque_margin_nm:a.planning.minimum_torque_margin_nm,
  maximum_point_penetration_m:a.planning.maximum_point_penetration_m,within_planning_tolerances:a.planning.within_planning_tolerances,
  geometry_samples:a.geometry.length,maximum_interlink_overlap_m:Math.max(...a.geometry.map(g=>g.maximum_inter_link_penetration_m))});
const rows=[];
for(const name of ['slip32-control','slip32-shaped','slip32-shaped05','slip32-shaped02']) {
  const recipe=read(name+'.recipe.json'),result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  for(const key of ['config','initial_positions','bounds','search','scaling_exponent','derivative_refinement'])
    assert(same(recipe[key],reference[key]),'unmatched trial '+key);
  if(recipe.slip_objective) {
    assert(same(recipe.slip_objective.point_groups,markers.map(m=>m.link)));
    assert.equal(recipe.slip_objective.target_ratio,.05);assert.equal(recipe.slip_objective.load_threshold_n,1);
  }
  assert(same(audit.planning,result.result.report),'independent physical report differs');
  assert(same(audit.slip,result.slip),'independent slip report differs');
  const byTime=new Map(dense.planning.frames.map(f=>[f.time_s,f]));
  audit.planning.frames.forEach(f=>assert(same(f,byTime.get(f.time_s)),'same-time physical frame differs'));
  const pointStride=recipe.config.contact_sliding_work_scale_j==null?1:3,pointRows=markers.length*pointStride;
  const motors=recipe.config.independent_coordinates.length,block=pointRows+6+2*motors+2*recipe.config.position_reference[0].length;
  const expected=[];let previous=0;
  for(let k=0;k<audit.planning.frames.length;k++) {
    const f=audit.planning.frames[k],weight=Math.sqrt(f.time_s-previous);previous=f.time_s;
    const r=audit.planning.residuals.slice(k*block,(k+1)*block);
    for(let j=0;j<markers.length;j++)expected.push(r[(j+1)*pointStride-1]/weight);
    expected.push(...r.slice(pointRows,pointRows+6).map(v=>v/weight));
    for(let j=0;j<motors;j++)expected.push(r[pointRows+6+2*j]/weight);
  }
  expected.push(...(result.slip?.shaping_residuals??[]));
  assert(same(expected,result.result.search.residuals),'optimized residual vector differs');
  const r=dense.planning,q=[r.periodic_boundary.initial_position,...r.frames.map(f=>f.position)];
  const T=r.frames.at(-1).time_s,dt=T/r.frames.length,D=Math.hypot(...r.periodic_boundary.translation_m.slice(0,2));
  const bodyPath=q.slice(1).reduce((s,p,k)=>s+Math.hypot(p[0]-q[k][0],p[1]-q[k][1]),0);
  const independent=links.map(link=>{
    let path=0,squared=0;const loads=[];
    for(const f of r.frames){let force=0,weightedSpeed=0,weightedSquared=0;
      markers.forEach((m,i)=>{if(m.link===link){const fn=f.contact_forces_world_n[i][2];const v=Math.hypot(...f.contact_velocities_world_m_s[i].slice(0,2));force+=fn;weightedSpeed+=fn*v;weightedSquared+=fn*v*v;}});
      loads.push(force);
      if(force>=1)path+=dt*weightedSpeed/force;
      squared+=dt*weightedSquared/Math.max(force,1);
    }
    const loaded=loads.map(f=>f>=1);
    return {group:link,actual:path/bodyPath,bound:Math.sqrt(T*squared)/D,loaded_fraction:loaded.filter(Boolean).length/loaded.length,
      touchdowns:loaded.filter((v,i)=>v&&!loaded[(i+loaded.length-1)%loaded.length]).length,
      liftoffs:loaded.filter((v,i)=>!v&&loaded[(i+loaded.length-1)%loaded.length]).length,
      minimum_normal_force_n:Math.min(...loads),maximum_normal_force_n:Math.max(...loads)};
  });
  let metricDifference=0;
  for(const g of dense.slip.groups){const i=independent.find(i=>i.group===g.group);assert(i);
    metricDifference=Math.max(metricDifference,Math.abs(i.actual-g.sampled_loaded_slip_ratio),Math.abs(i.bound-g.rms_slip_upper_bound));}
  assert(metricDifference<1e-12,'independent dense slip reduction differs');
  const positions=result.positions;
  const displacement=positions.at(-1).slice(0,2).map((v,j)=>v-positions[0][j]);
  recipe.bounds.at(-1).forEach((b,j)=>{assert.equal(b.lower,b.upper);assert(Math.abs(displacement[j]-b.lower)<1e-15,'fixed displacement changed');});
  const s=result.result.search;
  rows.push({name,slip_ratio_scale:recipe.slip_objective?.ratio_scale??null,
    optimized:metrics(audit),dense:metrics(dense),
    dense_slip: dense.slip,contact_pattern:independent,maximum_independent_slip_metric_difference:metricDifference,
    planned_displacement_rate_m_s:(displacement[0]+displacement[1])/Math.sqrt(2)/T,
    search:{termination:s.termination,evaluations:s.evaluations,initial_cost:s.initial_cost,cost:s.cost,last:s.history.at(-1)}});
}
console.log(JSON.stringify({scope:'Four matched fixed-displacement searches vary only optional slip shaping and its explicit scale. Physical and original loaded-slip gates are independently audited at 512 times; surrogate acceptance is not substituted for them. Objective costs with different weights are not directly comparable. No measured speed or physical maximum claim.',initial_parity:read('slip32-initial-parity.json'),rows},null,2));
