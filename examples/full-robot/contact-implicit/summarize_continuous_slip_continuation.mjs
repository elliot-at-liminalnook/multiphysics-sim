import fs from 'node:fs';import assert from 'node:assert/strict';import {isDeepStrictEqual as same} from 'node:util';
import {verifyInequalityTrial,verifyContinuousMeanSlipAudit} from './verify_inequality_evidence.mjs';
const root='examples/full-robot/contact-implicit/',read=n=>JSON.parse(fs.readFileSync(root+n));
const parentRecipe=read('mean68-restore.recipe.json'),parent=read('mean68-restore.result.json');
const control=read('mean68-warm.recipe.json'),jacobi=read('mean68-warm-jacobi.recipe.json');
for(const key of ['config','initial_positions','bounds','slip_objective','warm_start'])assert(same(control[key],jacobi[key]),'unmatched scaling comparison '+key);
const scaled=structuredClone(jacobi.search);assert.equal(scaled.scaling_exponent,.5);scaled.scaling_exponent=.25;
assert(same(scaled,control.search),'scaling comparison changed more than exponent');
const expectedSearch={...parentRecipe.search,maximum_outer_iterations:2};
assert(same(control.search,expectedSearch),'continuation changed inner settings');
for(const key of ['config','bounds','slip_objective'])assert(same(control[key],parentRecipe[key]),'continuation changed '+key);
assert(same(control.initial_positions,parent.positions),'continuation motion changed');
assert(same(control.warm_start,parent.result.search.continuation),'continuation checkpoint changed');
const trials=[];
for(const name of ['mean68-warm','mean68-warm-jacobi']) {
  const recipe=read(name+'.recipe.json'),result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  const v=verifyInequalityTrial(recipe,result,audit,dense),s=v.search;
  [audit,dense].forEach(a=>verifyContinuousMeanSlipAudit(recipe,a));
  const metrics=a=>({...v.metrics(a),maximum_continuous_mean_slip_bound:Math.max(...a.slip.groups.map(g=>g.continuous_mean_slip_upper_bound)),body_path_to_displacement_ratio:a.slip.body_path_m/a.slip.displacement_m});
  trials.push({name,scaling_exponent:recipe.search.scaling_exponent,coarse:metrics(audit),dense:metrics(dense),dense_slip:dense.slip,
    planned_projected_45deg_rate_m_s:v.displacement.reduce((s,v)=>s+v,0)/Math.sqrt(2)/dense.planning.frames.at(-1).time_s,
    search:{termination:s.termination,evaluations:s.evaluations,maximum_violation:s.maximum_violation,within_constraint_tolerance:s.within_constraint_tolerance,
      history:s.history.map(({inner,...h})=>({...h,inner_termination:inner.termination,inner_evaluations:inner.evaluations,rejected_evaluations:inner.rejected_evaluations,last_inner:inner.history.at(-1)})),next_penalty:s.continuation?.next_penalty??null}});
}
const prior=read('barrier68-summary.json').retained_warm68_dense,selected=trials[0].dense,other=trials[1].dense;
for(const candidate of [selected,other])assert(candidate.within_sampled_slip_limit&&candidate.maximum_continuous_mean_slip_bound<parentRecipe.slip_objective.target_ratio&&candidate.maximum_interlink_overlap_m===0,'retention requires sampled slip and collision passes');
for(const comparison of [prior,other])assert(selected.force_error_n<comparison.force_error_n&&selected.moment_error_nm<comparison.moment_error_nm&&selected.minimum_torque_margin_nm>comparison.minimum_torque_margin_nm,'retention balance/torque comparison failed');
assert(selected.maximum_loaded_slip_ratio<prior.maximum_loaded_slip_ratio,'retained slip did not improve');
console.log(JSON.stringify({scope:'Matched persistent continuous-mean-slip barrier continuations; only diagonal scaling differs. Shared native reports and independent dense metrics remain separate from runtime qualification and physical speed limits.',
  retained_numerical_seed:'mean68-warm',retention_reason:'Both dense slip gates pass; exponent 0.25 has lower dense force/moment errors and more torque margin than the matched Jacobi trial. It also improves these metrics and actual slip over the earlier warm68 seed. Balance still fails, so this is not runtime promotion.',
  checkpoint_exactly_preserved:true,initial_parent_dense:read('mean68-summary.json').continuous_dense,
  earlier_retained_warm68:prior,trials},null,2));
