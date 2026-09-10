// Reproduce the continued slip/balance comparison and identify geometry pairs.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const parent=read('slip32-shaped05.result.json');
const control=read('slip32-coupled-control.recipe.json');
const targeted=read('slip32-coupled-targeted.recipe.json');
for(const key of ['initial_positions','bounds','search','scaling_exponent','derivative_refinement','slip_objective'])
  assert(same(control[key],targeted[key]),'unmatched continuation '+key);
assert(same(control.initial_positions,parent.positions),'continuation parent differs');
const configWithoutGrid=c=>{const copy=structuredClone(c);delete copy.periodic_collocation_phases;return copy;};
assert(same(configWithoutGrid(control.config),configWithoutGrid(targeted.config)),'physical config differs');
const oldGrid=control.config.periodic_collocation_phases,newGrid=targeted.config.periodic_collocation_phases;
assert.equal(oldGrid.length,144);assert.equal(newGrid.length,176);
assert(oldGrid.every(t=>newGrid.includes(t)),'targeted grid dropped prior nodes');
assert.equal(targeted.provenance.selected.length,32);
const metrics=a=>({samples:a.planning.frames.length,force_error_n:a.planning.maximum_force_error_n,
  moment_error_nm:a.planning.maximum_moment_error_nm,minimum_torque_margin_nm:a.planning.minimum_torque_margin_nm,
  maximum_point_penetration_m:a.planning.maximum_point_penetration_m,within_planning_tolerances:a.planning.within_planning_tolerances,
  maximum_loaded_slip_ratio:Math.max(...a.slip.groups.map(g=>g.sampled_loaded_slip_ratio)),
  within_sampled_slip_limit:a.slip.within_sampled_slip_limit,
  geometry_samples:a.geometry.length,maximum_interlink_overlap_m:Math.max(...a.geometry.map(g=>g.maximum_inter_link_penetration_m))});
const rows=[];
for(const name of ['slip32-shaped05','slip32-coupled-control','slip32-coupled-targeted']) {
  const recipe=read(name+'.recipe.json'),result=read(name+'.result.json'),audit=read(name+'.audit.json');
  const dense=read(name==='slip32-shaped05'?'slip-coupled-parent-pairs.audit.json':name+'-dense.audit.json');
  assert(same(audit.planning,result.result.report),'independent physical report differs');
  assert(same(audit.slip,result.slip),'independent slip report differs');
  const byTime=new Map(dense.planning.frames.map(f=>[f.time_s,f]));
  audit.planning.frames.forEach(f=>assert(same(f,byTime.get(f.time_s)),'same-time physical frame differs'));
  const markerCount=recipe.slip_objective.point_groups.length;
  const pointStride=recipe.config.contact_sliding_work_scale_j==null?1:3,pointRows=markerCount*pointStride;
  const motors=recipe.config.independent_coordinates.length,block=pointRows+6+2*motors+2*recipe.config.position_reference[0].length;
  const expected=[];let previous=0;
  for(let k=0;k<audit.planning.frames.length;k++) {
    const f=audit.planning.frames[k],weight=Math.sqrt(f.time_s-previous);previous=f.time_s;
    const r=audit.planning.residuals.slice(k*block,(k+1)*block);
    for(let j=0;j<markerCount;j++)expected.push(r[(j+1)*pointStride-1]/weight);
    expected.push(...r.slice(pointRows,pointRows+6).map(v=>v/weight));
    for(let j=0;j<motors;j++)expected.push(r[pointRows+6+2*j]/weight);
  }
  expected.push(...result.slip.shaping_residuals);
  assert(same(expected,result.result.search.residuals),'optimized residual vector differs');
  const pairs=new Map();let overlappingFrames=0;
  for(const frame of dense.geometry) {
    assert(Array.isArray(frame.inter_link_penetrations),'missing detailed geometry');
    assert.equal(frame.maximum_inter_link_penetration_m,Math.max(0,...frame.inter_link_penetrations.map(p=>p.penetration_m)));
    if(frame.inter_link_penetrations.length)overlappingFrames++;
    for(const pair of frame.inter_link_penetrations) {
      const from=dense.link_names[pair.link],to=dense.link_names[pair.other];assert(from&&to);
      const key=JSON.stringify([from,to]);
      const row=pairs.get(key)??{from,to,samples:0,maximum_penetration_m:0};row.samples++;
      if(pair.penetration_m>row.maximum_penetration_m)Object.assign(row,{maximum_penetration_m:pair.penetration_m,time_s:frame.time_s,point_world_m:pair.point_m,normal_world:pair.normal_world});
      pairs.set(key,row);
    }
  }
  const displacement=result.positions.at(-1).slice(0,2).map((v,j)=>v-result.positions[0][j]);
  recipe.bounds.at(-1).forEach((b,j)=>{assert.equal(b.lower,b.upper);assert(Math.abs(displacement[j]-b.lower)<1e-15,'fixed displacement changed');});
  assert(same(dense.planning.periodic_boundary.translation_m,parent.result.report.periodic_boundary.translation_m),'planned displacement differs');
  const T=dense.planning.frames.at(-1).time_s,s=result.result.search;
  rows.push({name,optimized:metrics(audit),dense:metrics(dense),dense_slip:dense.slip,
    planned_projected_45deg_rate_m_s:(displacement[0]+displacement[1])/Math.sqrt(2)/T,
    collision:{overlapping_frames:overlappingFrames,pairs:[...pairs.values()]},
    search:{termination:s.termination,evaluations:s.evaluations,initial_cost:s.initial_cost,cost:s.cost}});
}
const oldParent=read('slip32-shaped05-dense.audit.json'),newParent=read('slip-coupled-parent-pairs.audit.json');
assert(same(oldParent.planning,newParent.planning),'parent physics changed');
assert(same(oldParent.slip,newParent.slip),'parent slip changed');
assert(same(oldParent.geometry,newParent.geometry.map(({inter_link_penetrations,...g})=>g)),'compact parent geometry changed');
console.log(JSON.stringify({scope:'Fixed-displacement continuation with identical initial motion, model, bounds and slip objective. Adding collocation nodes also increases the number of unweighted physical residuals relative to four aggregate slip residuals; this is not pure grid-only causal isolation. All trials fail qualification; projected planned rate is not measured walking speed or a physical bound.',rows},null,2));
