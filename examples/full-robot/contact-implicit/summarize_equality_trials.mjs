// Reduce native Rust evidence; this does not simulate physics.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const markers=read('surface-markers.json').markers;
const links=[...new Set(markers.map(m=>m.link))];
const metrics=a=>({samples:a.planning.frames.length,
  force_error_n:a.planning.maximum_force_error_n,
  moment_error_nm:a.planning.maximum_moment_error_nm,
  minimum_torque_margin_nm:a.planning.minimum_torque_margin_nm,
  maximum_point_penetration_m:a.planning.maximum_point_penetration_m,
  within_planning_tolerances:a.planning.within_planning_tolerances,
  geometry_samples:a.geometry.length,
  maximum_interlink_overlap_m:Math.max(...a.geometry.map(f=>f.maximum_inter_link_penetration_m)),
  maximum_floor_penetration_m:Math.max(0,-Math.min(...a.geometry.flatMap(f=>f.floor_clearances.map(c=>c.minimum_clearance_m))))});
const baseline=read('equality32.recipe.json');
const rows=[];
for(const name of ['equality32','equality32-whitened','equality32-exact']) {
  const recipe=read(name+'.recipe.json'),result=read(name+'.result.json');
  for(const key of ['config','initial_positions','bounds'])
    assert(isDeepStrictEqual(recipe[key],baseline[key]),'unmatched '+key);
  const audit=read(name+'.audit.json');
  const dense=read(name==='equality32'?'targeted32-recorded-dense.audit.json':name+'-dense.audit.json');
  assert(isDeepStrictEqual(audit.planning,result.result.report),'independent optimizer audit differs');
  if(name==='equality32') assert(isDeepStrictEqual(result.positions,baseline.initial_positions),'failed first solve changed motion');
  const byTime=new Map(dense.planning.frames.map(f=>[f.time_s,f]));
  audit.planning.frames.forEach(f=>assert(isDeepStrictEqual(f,byTime.get(f.time_s)),'same-time physics differs'));
  const r=dense.planning,q=[r.periodic_boundary.initial_position,...r.frames.map(f=>f.position)];
  const duration=r.frames.at(-1).time_s,dt=duration/r.frames.length;
  const bodyPath=q.slice(1).reduce((s,p,k)=>s+Math.hypot(p[0]-q[k][0],p[1]-q[k][1]),0);
  const loadedPaths=links.map(link=>r.frames.reduce((sum,f)=>{
    let load=0,speed=0;
    markers.forEach((m,i)=>{if(m.link===link){const fn=f.contact_forces_world_n[i][2];load+=fn;speed+=fn*Math.hypot(...f.contact_velocities_world_m_s[i].slice(0,2));}});
    return sum+(load>=1?dt*speed/load:0);
  },0));
  const s=result.result.search;
  rows.push({name,optimized:metrics(audit),dense:metrics(dense),
    planned_displacement_rate_m_s:(q.at(-1)[0]-q[0][0]+q.at(-1)[1]-q[0][1])/Math.sqrt(2)/duration,
    dense_loaded_slip_ratio:Math.max(...loadedPaths)/bodyPath,
    dense_sliding_work_j:r.contact_sliding_work_j,
    search:{initial_objective_cost:s.initial_objective_cost,objective_cost:s.objective_cost,
      initial_equality_inf:s.initial_equality_inf,equality_inf:s.equality_inf,
      evaluations:s.evaluations,termination:s.termination,linear_failure:s.linear_failure??null,
      accepted_steps:s.history.filter(h=>h.accepted).length,last:s.history.at(-1)??null}});
}
console.log(JSON.stringify({scope:'Same initial motion, CAD, bounds, contact law and 80 collocation samples. Independent 512-sample physical comparison; original failed solve uses identical parent dense audit. Planned displacement is not measured speed. No candidate is qualified.',rows},null,2));
