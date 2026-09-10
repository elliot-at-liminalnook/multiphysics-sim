// Reduce independent shared-Rust reports; do not calculate replacement physics.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const rows=[];
for(const mode of ['uniform','perturbed','recorded']) {
  const name='smooth8-'+mode,recipe=read(name+'.recipe.json');
  const result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  const summary=read(name+'.summary.json').stages.at(-1);
  assert(isDeepStrictEqual(result.stages.at(-1).result.report,audit.planning),'uncached report differs');
  assert(isDeepStrictEqual(audit.geometry,dense.geometry),'identical geometry grid differs');
  const coarse=audit.planning.frames, fine=dense.planning.frames;
  assert.equal(fine.length,4*coarse.length);
  coarse.forEach((f,k)=>assert(isDeepStrictEqual(f,fine[4*k+3]),'analytic physics differs at shared time'));
  const drift=audit.planning.periodic_boundary;
  assert.deepEqual(drift.initial_velocity,coarse.at(-1).velocity);
  const joints=recipe.config.independent_coordinates.map((name,j)=>{
    const speeds=fine.map(f=>f.velocity[6+j]);
    const signs=speeds.filter(v=>Math.abs(v)>1e-8).map(Math.sign);
    const reversals=signs.reduce((n,s,k)=>n+(s!==signs[(k+signs.length-1)%signs.length]?1:0),0);
    const values=fine.map(f=>f.position[6+j]);
    return{name,sampled_span_rad:Math.max(...values)-Math.min(...values),sampled_velocity_sign_changes:reversals};
  });
  const metrics=r=>({force_error_n:r.maximum_force_error_n,moment_error_nm:r.maximum_moment_error_nm,
    minimum_torque_margin_nm:r.minimum_torque_margin_nm,within_planning_tolerances:r.within_planning_tolerances});
  rows.push({mode,planned_displacement_rate_m_s:summary.planned_mean_diagonal_body_displacement_rate_m_s,
    loaded_slip_ratio:summary.maximum_planned_loaded_slip_ratio,coarse:metrics(audit.planning),dense:metrics(dense.planning),
    maximum_interlink_overlap_m:Math.max(...dense.geometry.map(f=>f.maximum_inter_link_penetration_m)),
    maximum_floor_penetration_m:-Math.min(...dense.geometry.flatMap(f=>f.floor_clearances.map(c=>c.minimum_clearance_m))),
    joints,termination:result.stages.at(-1).result.search.termination,cost:result.stages.at(-1).result.search.cost});
}
console.log(JSON.stringify({scope:'Analytic C2 periodic candidates: independent audit equality and exact physics agreement at shared sample times. Additional samples expose missed force/torque peaks. No runtime gait, stable orbit or physical speed ceiling is established.',
  controls:8,period_s:8*read('smooth8-uniform.recipe.json').config.step_s,coarse_samples:32,dense_samples:128,geometry_poses:129,rows},null,2));
