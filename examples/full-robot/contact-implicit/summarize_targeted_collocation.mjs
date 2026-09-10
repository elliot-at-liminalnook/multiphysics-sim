// Compare identical seeds/budgets on uniform vs targeted grids using independent
// 512-point Rust reports. This file reduces evidence, not replacement physics.
import fs from 'node:fs';import assert from 'node:assert/strict';import {isDeepStrictEqual} from 'node:util';
const root='examples/full-robot/contact-implicit/',read=n=>JSON.parse(fs.readFileSync(root+n));
const targeted=read('targeted32-recorded.recipe.json'),control=read('targeted32-control.recipe.json');
const stripped=r=>{r=structuredClone(r);delete r.provenance;delete r.config.periodic_collocation_phases;return r;};
assert(isDeepStrictEqual(stripped(targeted),stripped(control)),'comparison changes more than collocation grid');
const markers=read('surface-markers.json').markers,links=[...new Set(markers.map(m=>m.link))];
const metrics=a=>({samples:a.planning.frames.length,force_error_n:a.planning.maximum_force_error_n,moment_error_nm:a.planning.maximum_moment_error_nm,
  minimum_torque_margin_nm:a.planning.minimum_torque_margin_nm,within_planning_tolerances:a.planning.within_planning_tolerances,
  maximum_interlink_overlap_m:Math.max(...a.geometry.map(f=>f.maximum_inter_link_penetration_m)),
  maximum_floor_penetration_m:-Math.min(...a.geometry.flatMap(f=>f.floor_clearances.map(c=>c.minimum_clearance_m)))});
const rows=[];
for(const mode of ['control','recorded']) {
  const name='targeted32-'+mode,recipe=read(name+'.recipe.json'),result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  assert(isDeepStrictEqual(audit.planning,result.stages.at(-1).result.report),'independent optimizer audit differs');
  const byTime=new Map(dense.planning.frames.map(f=>[f.time_s,f]));
  audit.planning.frames.forEach(f=>assert(isDeepStrictEqual(f,byTime.get(f.time_s)),'same-time physics differs'));
  const r=dense.planning,q=[r.periodic_boundary.initial_position,...r.frames.map(f=>f.position)],duration=r.frames.at(-1).time_s,dt=duration/r.frames.length;
  const bodyPath=q.slice(1).reduce((s,p,k)=>s+Math.hypot(p[0]-q[k][0],p[1]-q[k][1]),0);
  const loadedPaths=links.map(link=>r.frames.reduce((sum,f)=>{let load=0,speed=0;markers.forEach((m,i)=>{if(m.link===link){const fn=f.contact_forces_world_n[i][2];load+=fn;speed+=fn*Math.hypot(...f.contact_velocities_world_m_s[i].slice(0,2));}});return sum+(load>=1?dt*speed/load:0);},0));
  const s=result.stages.at(-1).result.search;
  rows.push({mode,optimized:metrics(audit),dense:metrics(dense),planned_displacement_rate_m_s:(q.at(-1)[0]-q[0][0]+q.at(-1)[1]-q[0][1])/Math.sqrt(2)/duration,
    dense_loaded_slip_ratio:Math.max(...loadedPaths)/bodyPath,dense_sliding_work_j:r.contact_sliding_work_j,
    search:{initial_cost:s.initial_cost,cost:s.cost,evaluations:s.evaluations,termination:s.termination,derivative_refinements:s.derivative_refinements,last:s.history.at(-1)}});
}
const original=read('smooth32-recorded-512.audit.json');
console.log(JSON.stringify({scope:'Matched initial motion, bounds, physical model, objective scales and optimization budgets; collocation grid and corresponding time quadrature differ. Quality compared on the same independent 512-point grid. No measured robot speed or physical maximum claim.',baseline_dense:metrics(original),targeted_added_nodes:targeted.provenance.selected,rows},null,2));
