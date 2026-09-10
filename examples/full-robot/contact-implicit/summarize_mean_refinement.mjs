import fs from 'node:fs';import assert from 'node:assert/strict';import {isDeepStrictEqual as same} from 'node:util';
import {verifyInequalityTrial,verifyContinuousMeanSlipAudit} from './verify_inequality_evidence.mjs';
const root='examples/full-robot/contact-implicit/',read=n=>JSON.parse(fs.readFileSync(root+n));
const parent=read('mean68-warm.result.json'),control=read('mean32-inner60.recipe.json'),refined=read('mean64-inner60.recipe.json');
assert(same(control.warm_start,parent.result.search.continuation),'control checkpoint changed');
assert(same(control.initial_positions,parent.positions),'control initial motion changed');
assert(same(control.search,refined.search)&&same(control.slip_objective,refined.slip_objective),'comparison solver/objective changed');
const oldInspection=read('mean-refinement-parent-inspection.audit.json'),newInspection=read('mean64-inspection.audit.json');
const ordinary=structuredClone(oldInspection);delete ordinary.inequality_inspection;
assert(same(ordinary,read('mean68-warm.audit.json'))&&same(ordinary,read('mean-refinement-parent.audit.json')),'new audit CLI changed default output');
assert(same(oldInspection.inequality_inspection.values,control.warm_start.values)&&same(oldInspection.inequality_inspection.residuals,control.warm_start.residuals),'native parent inspection differs from checkpoint');
assert(same(refined.warm_start.values,newInspection.inequality_inspection.values)&&same(refined.warm_start.residuals,newInspection.inequality_inspection.residuals),'refined checkpoint does not use native inspection');
for(const key of ['multipliers','next_penalty','previous_shifted_norm','completed_outer_iterations'])assert(same(refined.warm_start[key],control.warm_start[key]),'dual migration changed '+key);
const oldConfig=structuredClone(control.config),newConfig=structuredClone(refined.config);
for(const key of ['step_s','position_reference','periodic_cubic_subdivisions']){delete oldConfig[key];delete newConfig[key];}
assert(same(oldConfig,newConfig),'physical config changed');
assert.equal(refined.config.step_s*2,control.config.step_s);assert.equal(refined.config.periodic_cubic_subdivisions*2,control.config.periodic_cubic_subdivisions);
const delta=(a,b)=>{if(typeof a==='number')return Math.abs(a-b);assert.equal(a.length,b.length);return a.reduce((m,v,j)=>Math.max(m,delta(v,b[j])),0);};
const initialComparison=(a,b)=>{
  assert.equal(a.planning.frames.length,b.planning.frames.length);const differences={};
  for(const [key,tolerance] of Object.entries(refined.provenance.physical_difference_tolerances)){
    differences[key]=Math.max(...a.planning.frames.map((f,k)=>{const g=b.planning.frames[k];assert.equal(f.time_s,g.time_s);return delta(f[key],g[key]);}));
    assert(differences[key]<tolerance,'initial curve mismatch '+key);
  }
  return {samples:a.planning.frames.length,differences};
};
const coarseInitial=initialComparison(oldInspection,newInspection),denseInitial=initialComparison(read('mean68-warm-dense.audit.json'),read('mean64-initial-dense.audit.json'));
verifyContinuousMeanSlipAudit(refined,newInspection);verifyContinuousMeanSlipAudit(refined,read('mean64-initial-dense.audit.json'));
const trials=[];
for(const name of ['mean32-inner60','mean64-inner60']) {
  const recipe=read(name+'.recipe.json'),result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  const v=verifyInequalityTrial(recipe,result,audit,dense),s=v.search;
  [audit,dense].forEach(a=>verifyContinuousMeanSlipAudit(recipe,a));
  const metrics=a=>({...v.metrics(a),maximum_continuous_mean_slip_bound:Math.max(...a.slip.groups.map(g=>g.continuous_mean_slip_upper_bound)),body_path_to_displacement_ratio:a.slip.body_path_m/a.slip.displacement_m});
  const bounds=recipe.bounds.flat(),active=[];let minDistance=Infinity,free=0;
  s.values.forEach((value,j)=>{const b=bounds[j],width=b.upper-b.lower;if(width>0){free++;const fraction=(value-b.lower)/width,distance=Math.min(fraction,1-fraction);minDistance=Math.min(minDistance,distance);if(distance<1e-8)active.push({parameter:j,value,lower:b.lower,upper:b.upper,fraction});}});
  const w=recipe.warm_start,penalty=w.next_penalty;
  const expectedInitialCost=.5*(w.residuals.objective.reduce((sum,r)=>sum+r*r,0)+w.residuals.inequalities.reduce((sum,g,j)=>sum+penalty*Math.max(g+w.multipliers[j]/penalty,0)**2,0));
  assert(Math.abs(expectedInitialCost-s.history[0].inner.initial_cost)<1e-9*Math.max(1,expectedInitialCost),'initial augmented cost mismatch');
  trials.push({name,controls:result.positions.length-1,parameters:s.values.length,free_parameters:free,coarse:metrics(audit),dense:metrics(dense),dense_slip:dense.slip,
    planned_projected_45deg_rate_m_s:v.displacement.reduce((s,x)=>s+x,0)/Math.sqrt(2)/dense.planning.frames.at(-1).time_s,
    numerical_bound_diagnostic:{threshold_fraction:1e-8,active_free_bounds:active,minimum_distance_fraction:minDistance},
    search:{termination:s.termination,evaluations:s.evaluations,maximum_violation:s.maximum_violation,within_constraint_tolerance:s.within_constraint_tolerance,
      history:s.history.map(({inner,...h})=>({...h,inner_termination:inner.termination,inner_evaluations:inner.evaluations,rejected_evaluations:inner.rejected_evaluations,initial_cost:inner.initial_cost,last_inner:inner.history.at(-1)})),next_penalty:s.continuation?.next_penalty??null}});
}
const selected=trials[1].dense,other=trials[0].dense;
assert(selected.force_error_n<other.force_error_n&&selected.moment_error_nm<other.moment_error_nm,'refined balance did not improve');
assert(selected.within_sampled_slip_limit&&selected.maximum_continuous_mean_slip_bound<refined.slip_objective.target_ratio&&selected.maximum_interlink_overlap_m===0&&selected.minimum_torque_margin_nm>=-refined.config.torque_tolerance_nm,'refined retention gates failed');
console.log(JSON.stringify({scope:'Exact initial-curve refinement with native coordinate/residual inspection and preserved physical-row multipliers. Equal inner iteration caps, different variable counts and evaluation cost. This does not prove the coarse basis infeasible or establish a physical maximum; dense audits and runtime qualification remain separate.',
  retained_numerical_seed:'mean64-inner60',retention_reason:'Lower dense force and moment errors while actual slip, mean bound, torque and sampled collision checks pass. Physical balance still fails; no runtime promotion.',
  default_audit_exact:true,coarse_initial_comparison:coarseInitial,dense_initial_comparison:denseInitial,
  native_checkpoint_migration_differences:refined.provenance.physical_differences,
  earlier_retained_seed:read('mean68-continuation-summary.json').trials[0].dense,trials},null,2));
