import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
import {verifyInequalityTrial} from './verify_inequality_evidence.mjs';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const parentRecipe=read('recorded68-restore.recipe.json'),parent=read('recorded68-restore.result.json');
const warm=read('warm68.recipe.json'),jacobi=read('warm68-jacobi.recipe.json');
for(const key of ['config','bounds','slip_objective'])assert(same(warm[key],parentRecipe[key]),'parent definition changed '+key);
assert(same(warm.initial_positions,parent.positions),'initial motion differs from parent result');
assert(same(warm.warm_start.values,parent.result.search.values),'parameter checkpoint differs');
assert(same(warm.warm_start.residuals,parent.result.search.residuals),'residual checkpoint differs');
assert(same(warm.warm_start.multipliers,parent.result.search.multipliers),'multipliers reset');
assert.equal(warm.warm_start.next_penalty,1);assert.equal(warm.warm_start.completed_outer_iterations,1);
for(const key of ['config','initial_positions','bounds','slip_objective','warm_start'])assert(same(warm[key],jacobi[key]),'scaling trial unmatched '+key);
const scaled=structuredClone(jacobi.search);scaled.scaling_exponent=warm.search.scaling_exponent;
assert(same(scaled,warm.search),'search differs beyond scaling');
assert(same(read('warm68-parent.audit.json'),read('recorded68-restore.audit.json')),'parent build parity differs');
const peaks=a=>['fx','fy','fz','mx','my','mz'].map((component,j)=>{
  const f=a.planning.frames.reduce((a,b)=>Math.abs(a.unactuated_wrench[j])>=Math.abs(b.unactuated_wrench[j])?a:b);
  return {component,value:f.unactuated_wrench[j],time_s:f.time_s};
});
let metrics;const rows=[];
for(const name of ['warm68','warm68-jacobi']) {
  const recipe=read(name+'.recipe.json'),result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  const v=verifyInequalityTrial(recipe,result,audit,dense);metrics=v.metrics;
  assert.equal(v.search.history[0].iteration,1);
  rows.push({name,scaling_exponent:recipe.search.scaling_exponent,coarse:metrics(audit),dense:metrics(dense),dense_wrench_peaks:peaks(dense),
    planned_projected_45deg_rate_m_s:v.displacement.reduce((s,v)=>s+v,0)/Math.sqrt(2)/dense.planning.frames.at(-1).time_s,
    search:{termination:v.search.termination,evaluations:v.search.evaluations,maximum_violation:v.search.maximum_violation,within_constraint_tolerance:v.search.within_constraint_tolerance,
      history:v.search.history.map(({inner,...h})=>({...h,inner_termination:inner.termination,inner_evaluations:inner.evaluations,last_inner:inner.history.at(-1)})),
      next_penalty:v.search.continuation?.next_penalty??null}});
}
console.log(JSON.stringify({scope:'Exact-state AL continuation and matched diagonal-scaling comparison at fixed recorded displacement. Actual sampled physics, slip and collision gates remain external to solver termination. No new measured speed, runtime controller promotion or physical maximum claim.',
  parent_dense:metrics(read('recorded68-restore-dense.audit.json')),parent_dense_wrench_peaks:peaks(read('recorded68-restore-dense.audit.json')),rows},null,2));
