// Shared evidence reduction; no dynamics or controller implementation.
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
// Reduce recorded native forces/velocities independently of the component.
// Recovered dt may differ by rounding from uniform native weights.
export function verifyContinuousMeanSlipAudit(recipe,audit) {
assert(recipe.slip_objective.use_continuous_mean_bound,'mean mode required');
for(const group of audit.slip.groups) {
  let previous=0,path=0;
  for(const frame of audit.planning.frames) {
    const dt=frame.time_s-previous;previous=frame.time_s;let load=0,weighted=0;
    assert(dt>0,'positive quadrature weight required');
    recipe.slip_objective.point_groups.forEach((name,j)=>{if(name===group.group){
      const force=frame.contact_forces_world_n[j][2],v=frame.contact_velocities_world_m_s[j];
      assert(Number.isFinite(force)&&force>=0,'nonnegative finite load required');
      load+=force;weighted+=force*Math.hypot(v[0],v[1]);
    }});
    path+=dt*weighted/Math.max(load,recipe.slip_objective.load_threshold_n);
  }
  const mean=path/audit.slip.body_path_m;
  assert(Math.abs(mean-group.continuous_mean_slip_upper_bound)<1e-12,'independent mean bound differs');
  assert(group.sampled_loaded_slip_ratio<=mean+1e-12&&mean<=group.rms_slip_upper_bound+1e-12,'independent bound ordering failed');
}
}
export function verifyInequalityTrial(recipe,result,audit,dense) {
assert(same(audit.planning,result.result.report),'independent physical report differs');
assert(same(audit.slip,result.slip),'independent slip report differs');
const byTime=new Map(dense.planning.frames.map(f=>[f.time_s,f]));
audit.planning.frames.forEach(f=>assert(same(f,byTime.get(f.time_s)),'dense same-time physics differs'));
const search=result.result.search,c=recipe.config;
const actualSlipRows=result.slip.groups.map(g=>(g.sampled_loaded_slip_ratio-recipe.slip_objective.target_ratio)/recipe.slip_objective.ratio_scale);
const weight=recipe.slip_objective.loaded_slip_barrier_weight;
const barrierRows=recipe.slip_objective.use_continuous_mean_bound
  ?result.slip.groups.map(g=>{
    const mean=g.continuous_mean_slip_upper_bound;
    assert(Number.isFinite(mean)&&g.sampled_loaded_slip_ratio<=mean+1e-10&&mean<=g.rms_slip_upper_bound+1e-10,'mean slip bound ordering failed');
    return (mean-recipe.slip_objective.target_ratio)/recipe.slip_objective.ratio_scale;
  }):actualSlipRows;
if(recipe.slip_objective.use_continuous_mean_bound)
  assert(same(result.slip.shaping_residuals,barrierRows.map(g=>Math.max(g,0))),'mean shaping residuals differ');
const expectedObjective=[...result.slip.shaping_residuals];
if(weight!==undefined&&weight!==null) {
  assert(Number.isFinite(weight)&&weight>0&&barrierRows.every(g=>Number.isFinite(g)&&g<0),'barrier domain violated');
  expectedObjective.push(...barrierRows.map(g=>Math.sqrt(weight)/-g));
}
assert(same(search.residuals.objective,expectedObjective),'slip objective differs');
const slipRows=recipe.slip_objective.constrain_loaded_slip
  ?result.slip.groups.map(g=>(g.sampled_loaded_slip_ratio-recipe.slip_objective.target_ratio)/recipe.slip_objective.ratio_scale):[];
const expectedMaximum=Math.max(0,audit.planning.maximum_force_error_n/c.force_tolerance_n-1,
  audit.planning.maximum_moment_error_nm/c.moment_tolerance_nm-1,
  -audit.planning.minimum_torque_margin_nm/c.torque_tolerance_nm-1,
  (audit.planning.maximum_point_penetration_m-c.maximum_point_penetration_m)/c.penetration_scale_m,...slipRows);
assert(Math.abs(expectedMaximum-search.maximum_violation)<1e-10,'inequality maximum differs from physical gates');
assert.equal(search.maximum_violation,Math.max(0,...search.residuals.inequalities));
assert.equal(search.within_constraint_tolerance,search.maximum_violation<=recipe.search.constraint_tolerance);
const points=recipe.slip_objective.point_groups.length,motors=c.independent_coordinates.length,stride=points+12+motors;
assert.equal(search.residuals.inequalities.length,audit.planning.frames.length*stride+slipRows.length);
assert(same(search.residuals.inequalities.slice(audit.planning.frames.length*stride),slipRows),'actual slip inequality rows differ');
for(let k=0;k<audit.planning.frames.length;k++) {
  const f=audit.planning.frames[k],r=search.residuals.inequalities.slice(k*stride,(k+1)*stride);
  f.gaps_m.forEach((g,j)=>assert.equal(r[j],(-g-c.maximum_point_penetration_m)/c.penetration_scale_m));
  f.unactuated_wrench.forEach((w,j)=>{const t=j<3?c.force_tolerance_n:c.moment_tolerance_nm;
    assert.equal(r[points+2*j],w/t-1);assert.equal(r[points+2*j+1],-w/t-1);});
  assert(Math.abs(Math.max(...r.slice(points+12))-(-f.minimum_torque_margin_nm/c.torque_tolerance_nm-1))<1e-10,'actuator inequalities differ from reported worst margin');
}
let multipliers=recipe.warm_start?.multipliers??Array(search.multipliers.length).fill(0);
for(const h of search.history) {
  assert(h.inner.evaluations<=recipe.search.inner.maximum_evaluations);
  if(h===search.history.at(-1)) {
    const expected=search.residuals.inequalities.map((g,j)=>Math.sqrt(h.penalty)*Math.max(g+multipliers[j]/h.penalty,0));
    const observed=h.inner.residuals.slice(search.residuals.objective.length);
    assert.equal(expected.length,observed.length);
    assert(Math.max(...expected.map((x,j)=>Math.abs(x-observed[j])))<1e-9,'last shifted residual differs');
  }
  multipliers=h.inner.residuals.slice(search.residuals.objective.length).map(r=>Math.sqrt(h.penalty)*r);
}
assert(Math.max(...multipliers.map((v,j)=>Math.abs(v-search.multipliers[j])))<1e-9,'multiplier update differs');
assert.equal(search.evaluations,1+search.history.reduce((s,h)=>s+h.inner.evaluations+1,0));
assert(search.evaluations<=recipe.search.maximum_evaluations);
if(search.continuation) {
  const next=search.continuation;
  assert.equal(search.termination,'outer_iteration_limit');
  assert(same(next.values,search.values)&&same(next.residuals,search.residuals)&&same(next.multipliers,search.multipliers),'continuation state differs from result');
  assert.equal(next.completed_outer_iterations,search.history.at(-1).iteration+1);
  let penalty=recipe.warm_start?.next_penalty??recipe.search.initial_penalty;
  let previous=recipe.warm_start?.previous_shifted_norm;
  // A resumed run explicitly supplies the preceding shifted norm, allowing
  // every next-penalty decision to be reproduced without a new model query.
  if(previous!==undefined) {
    for(const h of search.history) {
      assert.equal(h.penalty,penalty);
      if(h.shifted_constraint_norm>recipe.search.required_reduction*previous)
        penalty=Math.min(penalty*recipe.search.penalty_growth,recipe.search.maximum_penalty);
      previous=h.shifted_constraint_norm;
    }
    assert.equal(next.next_penalty,penalty);assert.equal(next.previous_shifted_norm,previous);
  }
}
const displacement=result.positions.at(-1).slice(0,2).map((v,j)=>v-result.positions[0][j]);
recipe.bounds.at(-1).forEach((b,j)=>{assert.equal(b.lower,b.upper);assert(Math.abs(displacement[j]-b.lower)<1e-15,'fixed displacement changed');});
const metrics=a=>({samples:a.planning.frames.length,force_error_n:a.planning.maximum_force_error_n,
  moment_error_nm:a.planning.maximum_moment_error_nm,minimum_torque_margin_nm:a.planning.minimum_torque_margin_nm,
  maximum_point_penetration_m:a.planning.maximum_point_penetration_m,within_planning_tolerances:a.planning.within_planning_tolerances,
  maximum_loaded_slip_ratio:Math.max(...a.slip.groups.map(g=>g.sampled_loaded_slip_ratio)),within_sampled_slip_limit:a.slip.within_sampled_slip_limit,
  geometry_samples:a.geometry.length,maximum_interlink_overlap_m:Math.max(...a.geometry.map(g=>g.maximum_inter_link_penetration_m))});
return {metrics,search,displacement};
}
