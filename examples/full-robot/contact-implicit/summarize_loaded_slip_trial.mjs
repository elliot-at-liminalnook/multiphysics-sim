import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as same} from 'node:util';
import {verifyInequalityTrial} from './verify_inequality_evidence.mjs';
const root='examples/full-robot/contact-implicit/';
const read=n=>JSON.parse(fs.readFileSync(root+n));
const control=read('warm68-jacobi.recipe.json'),recipe=read('sliplimit68-jacobi.recipe.json');
for(const key of ['config','initial_positions','bounds','search'])assert(same(recipe[key],control[key]),'unmatched slip-limit trial '+key);
const objective=structuredClone(recipe.slip_objective);delete objective.constrain_loaded_slip;
assert(same(objective,control.slip_objective),'objective differs beyond actual-slip flag');
const physicalRows=control.warm_start.residuals.inequalities.length;
assert(same(recipe.warm_start.values,control.warm_start.values),'initial parameters differ');
assert(same(recipe.warm_start.residuals.objective,control.warm_start.residuals.objective),'initial objective differs');
assert(same(recipe.warm_start.residuals.inequalities.slice(0,physicalRows),control.warm_start.residuals.inequalities),'old inequalities changed');
assert(same(recipe.warm_start.multipliers.slice(0,physicalRows),control.warm_start.multipliers),'old multipliers changed');
assert(recipe.warm_start.multipliers.slice(physicalRows).every(v=>v===0),'new multipliers are not zero');
assert(recipe.warm_start.residuals.inequalities.slice(physicalRows).every(v=>v<=0),'initial slip constraint does not pass');
for(const key of ['next_penalty','previous_shifted_norm','completed_outer_iterations'])assert.equal(recipe.warm_start[key],control.warm_start[key]);
const result=read('sliplimit68-jacobi.result.json'),audit=read('sliplimit68-jacobi.audit.json'),dense=read('sliplimit68-jacobi-dense.audit.json');
const v=verifyInequalityTrial(recipe,result,audit,dense),s=v.search;
const oldAudit=read('warm68-parent.audit.json'),newAudit=read('sliplimit68-parent.audit.json');
assert(same(oldAudit,newAudit),'new option changed parent physical/slip/geometry audit');
console.log(JSON.stringify({scope:'Matched actual-loaded-slip inequality experiment. Four appended rows start with zero multipliers; all previous physical constraints, objective, motion, grid, bounds, scaling and budgets match the earlier Jacobi control. Loaded-threshold and finite-grid effects remain; no runtime speed or physical maximum claim.',
  parent_audit_exactly_equal:true,control_dense:v.metrics(read('warm68-jacobi-dense.audit.json')),
  constrained_coarse:v.metrics(audit),constrained_dense:v.metrics(dense),dense_slip:dense.slip,
  final_actual_slip_inequalities:s.residuals.inequalities.slice(physicalRows),
  final_actual_slip_multipliers:s.multipliers.slice(physicalRows),
  planned_projected_45deg_rate_m_s:v.displacement.reduce((s,v)=>s+v,0)/Math.sqrt(2)/dense.planning.frames.at(-1).time_s,
  search:{termination:s.termination,evaluations:s.evaluations,maximum_violation:s.maximum_violation,within_constraint_tolerance:s.within_constraint_tolerance,
    history:s.history.map(({inner,...h})=>({...h,inner_termination:inner.termination,inner_evaluations:inner.evaluations,last_inner:inner.history.at(-1)})),next_penalty:s.continuation?.next_penalty??null}},null,2));
