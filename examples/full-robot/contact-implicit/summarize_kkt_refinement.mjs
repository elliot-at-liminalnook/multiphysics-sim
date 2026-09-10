// Evidence reduction only: native Rust supplies all physical frames.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const markers=read('surface-markers.json').markers;
const links=[...new Set(markers.map(m=>m.link))];
const oldRecipe=read('equality32-exact.recipe.json');
const newRecipe=read('equality32-refined-original.recipe.json');
for(const key of ['config','initial_positions','bounds','search'])
  assert(same(oldRecipe[key],newRecipe[key]),'original replay mismatch: '+key);
const oldResult=read('equality32-exact.result.json');
const newResult=read('equality32-refined-original.result.json');
let identicalHistoryPrefix=0;
for(let i=0;i<oldResult.result.search.history.length;i++) {
  const old=oldResult.result.search.history[i],next=newResult.result.search.history[i];
  if(!next || !Object.entries(old).every(([key,value])=>same(value,next[key]))) break;
  identicalHistoryPrefix++;
}
const control=read('equality32-refined-control.result.json');
const restart=read('equality32-refined.result.json');
assert(same(control.positions,restart.positions),'restart control positions differ');
assert(same(control.result.report,restart.result.report),'restart control physics differs');
const rows=[];
for(const name of ['equality32-exact','equality32-refined-original','equality32-refined']) {
  const result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  assert(same(audit.planning,result.result.report),'independent audit differs: '+name);
  const byTime=new Map(dense.planning.frames.map(f=>[f.time_s,f]));
  for(const f of audit.planning.frames) assert(same(f,byTime.get(f.time_s)),'same-time physics differs');
  const r=dense.planning,q=[r.periodic_boundary.initial_position,...r.frames.map(f=>f.position)];
  const duration=r.frames.at(-1).time_s,dt=duration/r.frames.length;
  const bodyPath=q.slice(1).reduce((s,p,k)=>s+Math.hypot(p[0]-q[k][0],p[1]-q[k][1]),0);
  const loadedPaths=links.map(link=>r.frames.reduce((sum,f)=>{
    let load=0,speed=0;
    markers.forEach((m,i)=>{if(m.link===link){const fn=f.contact_forces_world_n[i][2];load+=fn;speed+=fn*Math.hypot(...f.contact_velocities_world_m_s[i].slice(0,2));}});
    return sum+(load>=1?dt*speed/load:0);
  },0));
  const metrics=a=>({samples:a.planning.frames.length,force_error_n:a.planning.maximum_force_error_n,
    moment_error_nm:a.planning.maximum_moment_error_nm,minimum_torque_margin_nm:a.planning.minimum_torque_margin_nm,
    within_planning_tolerances:a.planning.within_planning_tolerances,
    maximum_interlink_overlap_m:Math.max(...a.geometry.map(g=>g.maximum_inter_link_penetration_m))});
  const s=result.result.search;
  rows.push({name,optimized:metrics(audit),dense:metrics(dense),
    planned_displacement_rate_m_s:(q.at(-1)[0]-q[0][0]+q.at(-1)[1]-q[0][1])/Math.sqrt(2)/duration,
    dense_loaded_slip_ratio:Math.max(...loadedPaths)/bodyPath,
    search:{termination:s.termination,evaluations:s.evaluations,initial_objective_cost:s.initial_objective_cost,
      objective_cost:s.objective_cost,equality_inf:s.equality_inf,linear_failure:s.linear_failure??null,
      refined_iterations:s.history.filter(h=>h.linear_diagnostics?.refinement_steps>0),last:s.history.at(-1)}});
}
console.log(JSON.stringify({scope:'Original recipe and eight-iteration restart comparisons. 512-sample physical audits; no qualified gait or measured speed gain.',
  identical_original_history_prefix:identicalHistoryPrefix,original_history_length:oldResult.result.search.history.length,
  restart_control_motion_and_physics_identical:true,rows},null,2));
