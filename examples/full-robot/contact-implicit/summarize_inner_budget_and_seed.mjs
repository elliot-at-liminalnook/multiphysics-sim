import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
import {verifyInequalityTrial} from './verify_inequality_evidence.mjs';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const original=read('inequality32.recipe.json'),longer=read('inequality32-inner30.recipe.json');
for(const key of ['config','initial_positions','bounds','slip_objective'])assert(same(original[key],longer[key]),'inner-budget input differs: '+key);
const adjusted=structuredClone(longer.search);adjusted.inner.maximum_iterations=original.search.inner.maximum_iterations;
assert(same(original.search,adjusted),'search differs beyond inner iteration cap');
assert.equal(original.search.inner.maximum_iterations,10);assert.equal(longer.search.inner.maximum_iterations,30);
const recorded=read('recorded68.recipe.json'),restore=read('recorded68-restore.recipe.json');
assert(same(recorded.initial_positions,restore.initial_positions),'recorded seed changed');
assert(same(recorded.bounds,restore.bounds),'recorded bounds changed');
assert(same(recorded.slip_objective,restore.slip_objective),'recorded slip objective changed');
const gridOnly=structuredClone(restore.config);gridOnly.periodic_collocation_phases=recorded.config.periodic_collocation_phases;
gridOnly.periodic_cubic_subdivisions=recorded.config.periodic_cubic_subdivisions;
assert(same(recorded.config,gridOnly),'warm-start physics differs beyond declared grid');
const initial=read('recorded68.initial.json');
const initialDense=read('recorded68-dense.audit.json');
assert.equal(initial.period_s,restore.config.step_s*(restore.initial_positions.length-1));
assert(same(initial.positions,restore.initial_positions),'sampler positions differ');
for(const [k,q] of initial.positions.slice(0,-1).entries())q.forEach((v,j)=>assert(v>=restore.bounds[k][j].lower&&v<=restore.bounds[k][j].upper,'seed outside bound'));
const rows=[];let metrics;
for(const name of ['inequality32-inner30','recorded68-restore']) {
  const recipe=read(name+'.recipe.json'),result=read(name+'.result.json'),audit=read(name+'.audit.json'),dense=read(name+'-dense.audit.json');
  const verified=verifyInequalityTrial(recipe,result,audit,dense);metrics=verified.metrics;
  const s=verified.search;
  rows.push({name,coarse:metrics(audit),dense:metrics(dense),
    planned_projected_45deg_rate_m_s:verified.displacement.reduce((s,v)=>s+v,0)/Math.sqrt(2)/dense.planning.frames.at(-1).time_s,
    search:{termination:s.termination,evaluations:s.evaluations,maximum_violation:s.maximum_violation,within_constraint_tolerance:s.within_constraint_tolerance,
      history:s.history.map(({inner,...h})=>({...h,inner_termination:inner.termination,inner_evaluations:inner.evaluations,last_inner:inner.history.at(-1)}))}});
}
console.log(JSON.stringify({scope:'A matched longer-inner experiment at the existing .201 planned rate, plus a separate recorded .068 numerical warm-start and first feasibility solve toward later speed continuation. Different initial motions and speeds are not an algorithm-only comparison. All reported rates are planned displacements, not new runtime speed measurements. No physical maximum claim.',
  original_short_inner_dense:metrics(read('inequality32-dense.audit.json')),
  recorded_seed_dense:metrics(initialDense),recorded_seed_slip:initialDense.slip,
  recorded_removed_endpoint_drift:initial.removed_endpoint_drift,rows},null,2));
