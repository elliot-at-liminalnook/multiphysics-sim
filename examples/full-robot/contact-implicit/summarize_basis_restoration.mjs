// Reduce shared Rust evidence and derive a conservative sampled slip bound.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const markers=read('surface-markers.json').markers,links=[...new Set(markers.map(m=>m.link))];
const a=read('restore32-targeted2.recipe.json'),b=read('restore64-targeted2.recipe.json');
assert(same(a.search,b.search),'search settings differ');
assert.equal(a.scaling_exponent,b.scaling_exponent);
assert(same(a.config.periodic_collocation_phases,b.config.periodic_collocation_phases),'check times differ');
assert.equal(a.config.step_s*(a.initial_positions.length-1),b.config.step_s*(b.initial_positions.length-1));
const physical=c=>{c=structuredClone(c);for(const k of ['step_s','position_reference','periodic_cubic_subdivisions'])delete c[k];return c;};
assert(same(physical(a.config),physical(b.config)),'physical model or objective references differ');
for(const recipe of [a,b])for(const row of recipe.bounds.slice(0,-1))
  for(let j=2;j<row.length;j++)assert(same(row[j],a.bounds[1][j]),'height/orientation/joint bound changed');
for(let j=0;j<2;j++) {
  assert.equal(a.bounds.at(-1)[j].lower,a.bounds.at(-1)[j].upper);
  assert.equal(b.bounds.at(-1)[j].lower,b.bounds.at(-1)[j].upper);
  assert(Math.abs(a.bounds.at(-1)[j].lower-b.bounds.at(-1)[j].lower)<1e-16,'displacement changed beyond rounding');
}
const metrics=a=>({samples:a.planning.frames.length,force_error_n:a.planning.maximum_force_error_n,
  moment_error_nm:a.planning.maximum_moment_error_nm,minimum_torque_margin_nm:a.planning.minimum_torque_margin_nm,
  maximum_point_penetration_m:a.planning.maximum_point_penetration_m,within_planning_tolerances:a.planning.within_planning_tolerances,
  geometry_samples:a.geometry.length,maximum_interlink_overlap_m:Math.max(...a.geometry.map(g=>g.maximum_inter_link_penetration_m))});
const rows=[];
for(const name of ['restore32-targeted','restore32-targeted2','restore64-targeted2']) {
  const result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  assert(same(audit.planning,result.result.report),'independent report differs');
  const byTime=new Map(dense.planning.frames.map(f=>[f.time_s,f]));
  audit.planning.frames.forEach(f=>assert(same(f,byTime.get(f.time_s)),'same-time physics differs'));
  const r=dense.planning,q=[r.periodic_boundary.initial_position,...r.frames.map(f=>f.position)];
  const T=r.frames.at(-1).time_s,dt=T/r.frames.length;
  const D=Math.hypot(...r.periodic_boundary.translation_m.slice(0,2));assert(D>0);
  const bodyPath=q.slice(1).reduce((s,p,k)=>s+Math.hypot(p[0]-q[k][0],p[1]-q[k][1]),0);
  assert(bodyPath+1e-12>=D);
  const slip=links.map(link=>{
    let loadedPath=0,squaredSpeedIntegral=0;
    for(const f of r.frames){
      let load=0,weightedSpeed=0,weightedSquaredSpeed=0;
      markers.forEach((m,i)=>{if(m.link===link){
        const fn=f.contact_forces_world_n[i][2];assert(fn>=0);
        const speed=Math.hypot(...f.contact_velocities_world_m_s[i].slice(0,2));
        load+=fn;weightedSpeed+=fn*speed;weightedSquaredSpeed+=fn*speed*speed;
      }});
      if(load>=1)loadedPath+=dt*weightedSpeed/load;
      squaredSpeedIntegral+=dt*weightedSquaredSpeed/Math.max(load,1);
    }
    const measured=loadedPath/bodyPath,bound=Math.sqrt(T*squaredSpeedIntegral)/D;
    assert(measured<=bound+1e-12,'derived sampled slip upper bound violated');
    return {link,sampled_loaded_slip_ratio:measured,sampled_rms_slip_upper_bound:bound};
  });
  const s=result.result.search;
  rows.push({name,optimized:metrics(audit),dense:metrics(dense),
    planned_displacement_rate_m_s:r.periodic_boundary.translation_m.slice(0,2).reduce((s,v)=>s+v,0)/Math.sqrt(2)/T,
    sampled_loaded_slip_ratio:Math.max(...slip.map(s=>s.sampled_loaded_slip_ratio)),
    sampled_rms_slip_upper_bound:Math.max(...slip.map(s=>s.sampled_rms_slip_upper_bound)),slip_by_foot:slip,
    search:{termination:s.termination,evaluations:s.evaluations,initial_cost:s.initial_cost,cost:s.cost,last:s.history.at(-1)}});
}
console.log(JSON.stringify({scope:'32 versus 64 exact-refined controls on identical 144 planning times, with 512-point physical audits and fixed displacement up to documented roundoff. A Cauchy-Schwarz upper bound applies to the sampled loaded-slip diagnostic, not unsampled time or hardware. No measured speed or global maximum claim.',initial_curve_parity:read('restore64-targeted2-initial-parity.json'),rows},null,2));
