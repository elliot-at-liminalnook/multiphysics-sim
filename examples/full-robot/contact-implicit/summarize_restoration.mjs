// Reduce recorded Rust physics and optimizer reports; no replacement simulation.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const markers=read('surface-markers.json').markers;
const links=[...new Set(markers.map(m=>m.link))];
const fixed=read('restore32-fixed.recipe.json'),full=read('restore32-full-control.recipe.json');
for(const key of ['config','initial_positions','bounds','search'])assert(same(fixed[key],full[key]),'unmatched full control '+key);
assert.equal(fixed.scaling_exponent,full.hessian_scaling_exponent);
const continued=read('restore32-continued.recipe.json'),targeted=read('restore32-targeted.recipe.json');
for(const key of ['initial_positions','bounds','search','scaling_exponent','derivative_refinement'])assert(same(continued[key],targeted[key]),'unmatched targeted control '+key);
const strip=c=>{c=structuredClone(c);delete c.periodic_collocation_phases;return c;};
assert(same(strip(continued.config),strip(targeted.config)),'targeted physical model differs');
assert(continued.config.periodic_collocation_phases.every(p=>targeted.config.periodic_collocation_phases.includes(p)),'targeting removed an existing node');
const metrics=a=>({samples:a.planning.frames.length,force_error_n:a.planning.maximum_force_error_n,
  moment_error_nm:a.planning.maximum_moment_error_nm,minimum_torque_margin_nm:a.planning.minimum_torque_margin_nm,
  maximum_point_penetration_m:a.planning.maximum_point_penetration_m,
  within_planning_tolerances:a.planning.within_planning_tolerances,
  geometry_samples:a.geometry.length,
  maximum_interlink_overlap_m:Math.max(...a.geometry.map(g=>g.maximum_inter_link_penetration_m)),
  maximum_floor_penetration_m:Math.max(0,-Math.min(...a.geometry.flatMap(g=>g.floor_clearances.map(f=>f.minimum_clearance_m))))});
const rows=[];
for(const name of ['restore32-full-control','restore32-fixed','restore32-continued','restore32-targeted']) {
  const recipe=read(name+'.recipe.json'),result=read(name+'.result.json');
  const optimization=result.result??result.stages.at(-1).result;
  const audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  assert(same(audit.planning,optimization.report),'independent report differs '+name);
  const byTime=new Map(dense.planning.frames.map(f=>[f.time_s,f]));
  audit.planning.frames.forEach(f=>assert(same(f,byTime.get(f.time_s)),'same-time physical frame differs'));
  const displacement=result.positions.at(-1).slice(0,2).map((v,j)=>v-result.positions[0][j]);
  recipe.bounds.at(-1).forEach((b,j)=>{
    assert.equal(b.lower,b.upper,'cycle displacement is not fixed');
    assert(Math.abs(displacement[j]-b.lower)<1e-15,'fixed displacement changed');
  });
  if(result.result) {
    const pointStride=recipe.config.contact_sliding_work_scale_j==null?1:3;
    const pointRows=markers.length*pointStride,motors=recipe.config.independent_coordinates.length;
    const block=pointRows+6+2*motors+2*recipe.config.position_reference[0].length;
    const expected=[];let previous=0;
    for(let k=0;k<audit.planning.frames.length;k++) {
      const f=audit.planning.frames[k],weight=Math.sqrt(f.time_s-previous);previous=f.time_s;
      const row=audit.planning.residuals.slice(k*block,(k+1)*block);
      for(let j=0;j<markers.length;j++)expected.push(row[(j+1)*pointStride-1]/weight);
      expected.push(...row.slice(pointRows,pointRows+6).map(v=>v/weight));
      for(let j=0;j<motors;j++)expected.push(row[pointRows+6+2*j]/weight);
    }
    assert(same(expected,optimization.search.residuals),'restoration partition differs '+name);
  }
  const r=dense.planning,q=[r.periodic_boundary.initial_position,...r.frames.map(f=>f.position)];
  const duration=r.frames.at(-1).time_s,dt=duration/r.frames.length;
  const bodyPath=q.slice(1).reduce((s,p,k)=>s+Math.hypot(p[0]-q[k][0],p[1]-q[k][1]),0);
  const loadedPaths=links.map(link=>r.frames.reduce((sum,f)=>{
    let load=0,speed=0;
    markers.forEach((m,i)=>{if(m.link===link){const fn=f.contact_forces_world_n[i][2];load+=fn;speed+=fn*Math.hypot(...f.contact_velocities_world_m_s[i].slice(0,2));}});
    return sum+(load>=1?dt*speed/load:0);
  },0));
  const s=optimization.search;
  rows.push({name,optimized:metrics(audit),dense:metrics(dense),
    planned_displacement_rate_m_s:(displacement[0]+displacement[1])/Math.sqrt(2)/duration,
    dense_loaded_slip_ratio:Math.max(...loadedPaths)/bodyPath,dense_sliding_work_j:r.contact_sliding_work_j,
    search:{termination:s.termination,evaluations:s.evaluations,initial_cost:s.initial_cost,cost:s.cost,last:s.history.at(-1)}});
}
const baseline=read('equality32-refined-original-dense.audit.json');
console.log(JSON.stringify({scope:'Same CAD, period and fixed displacement. Full-objective versus feasibility-only LM comparison, then matched continued/targeted restoration. Dense physical quality is compared independently; objective costs have different definitions. No measured speed, qualified gait or physical maximum claim.',baseline_dense:metrics(baseline),rows},null,2));
