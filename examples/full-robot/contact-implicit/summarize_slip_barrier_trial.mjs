import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
import {verifyInequalityTrial} from './verify_inequality_evidence.mjs';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const control=read('sliplimit68-jacobi.recipe.json'),recipe=read('barrier68-jacobi.recipe.json');
for(const key of ['config','initial_positions','bounds','search'])assert(same(recipe[key],control[key]),'unmatched barrier trial '+key);
const objective=structuredClone(recipe.slip_objective),weight=objective.loaded_slip_barrier_weight;
delete objective.loaded_slip_barrier_weight;
assert(same(objective,control.slip_objective),'objective differs beyond barrier weight');
const old=control.warm_start,w=recipe.warm_start;
for(const key of ['values','multipliers','next_penalty','previous_shifted_norm','completed_outer_iterations'])
  assert(same(w[key],old[key]),'checkpoint changed '+key);
assert(same(w.residuals.inequalities,old.residuals.inequalities),'initial inequalities changed');
const groups=[...new Set(recipe.slip_objective.point_groups)].sort();
const rows=old.residuals.inequalities.slice(-groups.length);
assert(rows.every(g=>g<0),'initial barrier state not interior');
assert(same(w.residuals.objective,[...old.residuals.objective,...rows.map(g=>Math.sqrt(weight)/-g)]),'initial barrier objective differs');
const result=read('barrier68-jacobi.result.json'),audit=read('barrier68-jacobi.audit.json'),dense=read('barrier68-jacobi-dense.audit.json');
const v=verifyInequalityTrial(recipe,result,audit,dense),s=v.search;
assert(same(read('sliplimit68-parent.audit.json'),read('barrier68-parent.audit.json')),'parent audit changed with new build');
for(const h of s.history) {
  const barrier=h.inner.residuals.slice(result.slip.shaping_residuals.length,s.residuals.objective.length);
  assert.equal(barrier.length,groups.length);
  assert(barrier.every(r=>Number.isFinite(r)&&r>0),'invalid barrier residual at outer endpoint');
}
const loadAt=(a,group,time)=>{
  const frame=a.planning.frames.find(f=>f.time_s===time);assert(frame,'missing diagnostic frame');
  return recipe.slip_objective.point_groups.reduce((sum,g,j)=>sum+(g===group?frame.contact_forces_world_n[j][2]:0),0);
};
const closest=[];
for(const f of audit.planning.frames)for(const group of groups){const load=loadAt(audit,group,f.time_s);
  closest.push({group,time_s:f.time_s,load_n:load,distance_to_threshold_n:load-recipe.slip_objective.load_threshold_n});}
closest.sort((a,b)=>Math.abs(a.distance_to_threshold_n)-Math.abs(b.distance_to_threshold_n));
const witness=closest[0],probes={};
for(const name of ['up','down']) {
  const path=read('barrier68-threshold-'+name+'.positions.json'),a=read('barrier68-threshold-'+name+'.audit.json');
  const delta=path.provenance.vertical_shift_m;
  assert.equal(delta,name==='up'?1e-12:-1e-12);
  assert(same(path.positions,result.positions.map(q=>q.map((x,j)=>j===2?x+delta:x))),'diagnostic changed more than vertical translation');
  probes[name]={vertical_shift_m:delta,load_n:loadAt(a,witness.group,witness.time_s),
    sampled_loaded_slip_ratio:a.slip.groups.find(g=>g.group===witness.group).sampled_loaded_slip_ratio,
    rms_slip_upper_bound:a.slip.groups.find(g=>g.group===witness.group).rms_slip_upper_bound,
    force_error_n:a.planning.maximum_force_error_n,moment_error_nm:a.planning.maximum_moment_error_nm};
}
assert(probes.up.load_n<recipe.slip_objective.load_threshold_n&&probes.down.load_n>=recipe.slip_objective.load_threshold_n,'probes did not cross the cutoff');
assert(probes.up.sampled_loaded_slip_ratio<recipe.slip_objective.target_ratio&&probes.down.sampled_loaded_slip_ratio>recipe.slip_objective.target_ratio,'cutoff crossing did not cross slip gate');
// Diagnostic reduction of recorded forces/velocities only. This candidate
// continuous bound is not yet implemented in a Rust optimizer or controller.
const meanBoundDiagnostic=[];
for(const file of ['recorded68.audit.json','recorded68-restore.audit.json','warm68.audit.json','barrier68-jacobi.audit.json','barrier68-jacobi-dense.audit.json']) {
  const input=read(file.replace('.audit.json','.recipe.json'));
  assert(same(input.slip_objective.point_groups,recipe.slip_objective.point_groups),'mean-bound diagnostic point grouping changed');
  assert.equal(input.slip_objective.load_threshold_n,recipe.slip_objective.load_threshold_n);
  const a=read(file),sums=groups.map(()=>0);let previous=0;
  for(const f of a.planning.frames) {
    const dt=f.time_s-previous;previous=f.time_s;
    groups.forEach((group,k)=>{
      let load=0,weighted=0;
      recipe.slip_objective.point_groups.forEach((g,j)=>{if(g===group){
        const force=f.contact_forces_world_n[j][2],v=f.contact_velocities_world_m_s[j];
        assert(Number.isFinite(force)&&force>=0,'nonnegative finite normal load required');
        load+=force;weighted+=force*Math.hypot(v[0],v[1]);
      }});
      sums[k]+=dt*weighted/Math.max(load,recipe.slip_objective.load_threshold_n)/a.slip.body_path_m;
    });
  }
  const report=groups.map((group,k)=>{
    const slip=a.slip.groups.find(g=>g.group===group);
    assert(slip.sampled_loaded_slip_ratio<=sums[k]+1e-12&&sums[k]<=slip.rms_slip_upper_bound+1e-12,'mean bound ordering failed');
    return {group,actual_slip:slip.sampled_loaded_slip_ratio,continuous_mean_bound:sums[k],rms_bound:slip.rms_slip_upper_bound};
  });
  meanBoundDiagnostic.push({source:file,samples:a.planning.frames.length,groups:report,maximum_continuous_mean_bound:Math.max(...sums)});
}
console.log(JSON.stringify({scope:'Matched strict sampled-slip barrier trial at fixed displacement. Solver-domain rejection protects only the optimization samples. Dense slip, balance, CAD collision and runtime tracking remain separate gates; no physical speed ceiling or qualified gait is implied.',
  parent_audit_exactly_equal:true,barrier_weight:weight,
  control_dense:v.metrics(read('sliplimit68-jacobi-dense.audit.json')),
  barrier_coarse:v.metrics(audit),barrier_dense:v.metrics(dense),dense_slip:dense.slip,
  retained_warm68_dense:v.metrics(read('warm68-dense.audit.json')),
  final_actual_slip_inequalities:s.residuals.inequalities.slice(-groups.length),
  final_barrier_residuals:s.residuals.objective.slice(result.slip.shaping_residuals.length),
  load_cutoff_diagnostic:{scope:'Numerical sensitivity experiment, not claimed physical positioning accuracy. Shared Rust audits of uniform vertical control perturbations show a hard loaded-set cutoff crossing.',witness,probes,
    slip_jump:probes.down.sampled_loaded_slip_ratio-probes.up.sampled_loaded_slip_ratio},
  continuous_mean_bound_diagnostic:{scope:'Offline reduction of recorded Rust forces and material velocities. Sum dt*sum(fn*|vt|)/max(group_load,threshold), divided by body path. Right-endpoint weights recovered from audit times; floating rounding may differ from native uniform dt. Actual <= mean bound <= RMS bound is checked, but this bound is not yet integrated into the optimizer.',audits:meanBoundDiagnostic},
  planned_projected_45deg_rate_m_s:v.displacement.reduce((s,v)=>s+v,0)/Math.sqrt(2)/dense.planning.frames.at(-1).time_s,
  search:{termination:s.termination,evaluations:s.evaluations,maximum_violation:s.maximum_violation,within_constraint_tolerance:s.within_constraint_tolerance,
    history:s.history.map(({inner,...h})=>({...h,inner_termination:inner.termination,inner_evaluations:inner.evaluations,rejected_evaluations:inner.rejected_evaluations,last_inner:inner.history.at(-1)})),next_penalty:s.continuation?.next_penalty??null}},null,2));
