// Evidence reduction only; Rust supplies trajectory dynamics, forces and geometry.
import fs from 'node:fs';import assert from 'node:assert/strict';import {isDeepStrictEqual} from 'node:util';
const root='examples/full-robot/contact-implicit/',read=n=>JSON.parse(fs.readFileSync(root+n));
const names=['periodic-uniform','periodic-perturbed','periodic-perturbed-refinement'];
const reports=names.map(name=>{
  const recipe=read(name+'.recipe.json'),result=read(name+'.result.json'),audit=read(name+'.audit.json'),summary=read(name+'.summary.json');
  const final=result.stages.at(-1).result,q=final.positions,r=final.report,s=summary.stages.at(-1);
  assert(recipe.config.periodic_horizontal_translation && recipe.config.initial_velocity.length===0);
  assert(isDeepStrictEqual(r,audit.planning),'uncached periodic audit mismatch: '+name);
  assert(isDeepStrictEqual(q[0].slice(2),q.at(-1).slice(2)),'periodic pose closure');
  assert(isDeepStrictEqual(r.periodic_boundary.initial_velocity,r.frames.at(-1).velocity),'periodic velocity closure');
  const variation=recipe.config.independent_coordinates.map((name,j)=>{
    const p=q.map(q=>q[6+j]),delta=p.slice(1).map((v,k)=>v-p[k]);let reversals=0;
    for(let k=0;k<delta.length;k++)if(delta[k]*delta[(k+1)%delta.length]<-1e-18)reversals++;
    return{name,span_rad:Math.max(...p)-Math.min(...p),total_variation_rad:delta.reduce((s,v)=>s+Math.abs(v),0),direction_reversals:reversals};
  });
  return{name,period_s:recipe.config.step_s*(q.length-1),exact_pose_and_velocity_closure:true,
    within_planning_tolerances:r.within_planning_tolerances,planned_diagonal_displacement_rate_m_s:s.planned_mean_diagonal_body_displacement_rate_m_s,
    maximum_force_error_n:r.maximum_force_error_n,maximum_moment_error_nm:r.maximum_moment_error_nm,
    minimum_torque_margin_nm:r.minimum_torque_margin_nm,maximum_planned_slip_ratio:s.maximum_planned_loaded_slip_ratio,
    geometry:summary.final_geometry,final_cost:final.search.cost,termination:final.search.termination,
    optimistic_total_peak_motoring_power_w:s.actuator_work_diagnostics.reduce((sum,a)=>sum+a.optimistic_peak_motoring_power_w,0),
    positive_discrete_motor_work_j:s.actuator_work_diagnostics.reduce((sum,a)=>sum+a.positive_discrete_work_j,0),
    maximum_individual_peak_power_ratio:Math.max(...s.actuator_work_diagnostics.map(a=>a.maximum_motoring_power_w/a.optimistic_peak_motoring_power_w)),
    knot_variation:variation};
});
console.log(JSON.stringify({reports,scope:'Infeasible periodic planning candidates, not measured speed, stable gait, continuous energy balance or a global physical limit. Reversal count ignores adjacent increment products smaller than 1e-18 rad squared.'},null,2));
