import {verifyInequalityTrial} from './verify_inequality_evidence.mjs';
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const recipe=read('inequality32.recipe.json'),control=read('slip32-coupled-control.recipe.json');
for(const key of ['config','initial_positions','bounds','slip_objective'])
  assert(same(recipe[key],control[key]),'unmatched comparison '+key);
assert.equal(recipe.search.scaling_exponent,control.scaling_exponent);
assert.equal(recipe.search.maximum_outer_iterations*recipe.search.inner.maximum_iterations,control.search.maximum_iterations);
for(const key of ['difference_step','initial_damping','gradient_tolerance'])assert.equal(recipe.search.inner[key],control.search[key]);
const parent=read('slip32-shaped05.audit.json'),initial=read('inequality32-initial.audit.json');
assert(same(parent.planning,initial.planning),'initial physical report changed');
assert(same(parent.slip,initial.slip),'initial slip changed');
assert(same(parent.geometry,initial.geometry.map(({poses,inter_link_penetrations,...g})=>g)),'initial compact geometry changed');
const result=read('inequality32.result.json'),audit=read('inequality32.audit.json'),dense=read('inequality32-dense.audit.json');
const {metrics,search,displacement}=verifyInequalityTrial(recipe,result,audit,dense);
console.log(JSON.stringify({scope:'Same starting motion, physical model, 144 check times, slip objective and fixed displacement as the weighted control. Three restarted ten-iteration AL subproblems differ from one thirty-iteration weighted solve; iteration counts are not equal computational cost. No qualified gait, runtime speed or global maximum claim.',
  initial_physical_and_slip_parity:true,planned_projected_45deg_rate_m_s:(displacement[0]+displacement[1])/Math.sqrt(2)/dense.planning.frames.at(-1).time_s,
  weighted_control_dense:metrics(read('slip32-coupled-control-dense.audit.json')),inequality_coarse:metrics(audit),inequality_dense:metrics(dense),
  search:{termination:search.termination,evaluations:search.evaluations,maximum_violation:search.maximum_violation,within_constraint_tolerance:search.within_constraint_tolerance,
    history:search.history.map(({inner,...h})=>({...h,inner_termination:inner.termination,inner_evaluations:inner.evaluations,inner_initial_cost:inner.initial_cost,inner_cost:inner.cost}))}},null,2));
