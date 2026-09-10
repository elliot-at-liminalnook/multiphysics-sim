import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning/';
const read=n=>JSON.parse(fs.readFileSync(d+n));
const same=(a,b,label)=>assert(isDeepStrictEqual(JSON.parse(JSON.stringify(a)),JSON.parse(JSON.stringify(b))),label);
const old=read('joint-ipopt-warm-close-one-step.result.json'),now=read('joint-body-grouping-one-step.result.json');
for(const field of ['candidate','report','initial_report','returned_candidate_error','search','best_sampled_feasible','jacobian_nonzeros','dense_jacobian_entries','constraint_projection']) same(now[field],old[field],`Grouped pilot changed ${field}`);
assert.equal(now.model_evaluations,220);assert.equal(old.model_evaluations,316);
assert.equal(now.grouped_body_probes,96);assert.equal(now.body_group_fallbacks,0);
const budget=read('joint-body-grouping-budget.result.json');
assert.equal(budget.model_evaluations,12);assert(budget.model_budget_exhausted);assert(budget.report);
assert.equal(budget.returned_candidate_error,null);
const fallback=read('joint-body-grouping-fallback.result.json');
assert(fallback.body_group_fallbacks>0,'Failure fixture did not exercise grouped fallback');
assert(fallback.model_evaluations<=180);assert(fallback.report);
assert.equal(fallback.returned_candidate_error,null);
for(const result of [now,budget,fallback]) {
 if(result.search.final_evaluation) {
  same(result.search.final_evaluation.constraints,result.report.constraints.inequalities,'Native final differs from uncached CAD');
  assert.equal(result.search.final_evaluation.objective,.5*result.report.constraints.objective[0]**2);
 } else {
  assert(result.model_budget_exhausted,'Missing native final callback without model budget exhaustion');
  assert.match(result.search.last_callback_error,/budget exhausted/);
  assert.equal(result.report.constraints.inequalities.length,6776);
 }
}
const identity=n=>{const path=d+n,bytes=fs.readFileSync(path);return {path,bytes:bytes.length,sha256:crypto.createHash('sha256').update(bytes).digest('hex')};};
const summary=r=>({model_evaluations:r.model_evaluations,grouped_body_probes:r.grouped_body_probes,body_group_fallbacks:r.body_group_fallbacks,native_status:r.search.native_status,model_budget_exhausted:r.model_budget_exhausted,speed_m_s:r.report.motion_report.speed_m_s,maximum_inequality:Math.max(...r.report.constraints.inequalities),feasible:r.report.sampled_feasible});
const result={native_search_and_returned_cad_identical:true,original_model_evaluations:316,grouped_model_evaluations:220,model_attempt_reduction_fraction:96/316,grouped:summary(now),budget:summary(budget),fallback:summary(fallback),artifacts:['joint-body-grouping-one-step.result.json','joint-body-grouping-budget.result.json','joint-body-grouping-fallback.result.json','check_joint_body_grouping.mjs'].map(identity),scope:'Same native pilot result and full physical audit with fewer model attempts, plus budget and deliberately unreachable-probe fallback checks. No optimizer wall-time speedup, new runtime gait, global maximum or browser qualification claimed.'};
fs.writeFileSync(d+'joint-body-grouping-verification.json',JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({...result,artifacts:undefined}));
