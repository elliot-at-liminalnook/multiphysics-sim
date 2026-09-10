import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const d='examples/full-robot/contact-planning/';
const read=p=>JSON.parse(fs.readFileSync(d+p));
assert(fs.readFileSync(d+'joint-derivative-one-step-old.result.json').equals(fs.readFileSync(d+'joint-derivative-one-step-new.result.json')),'Default solver regression');
const audit=read('joint-force-jacobian.result.json');
assert(isDeepStrictEqual(audit.reference_report,read('joint-x25-warm-initial.result.json')),'Original CAD report changed');
assert.equal(audit.cases.length,3);
for(const r of audit.cases){assert.equal(r.checked_force_columns,120);assert.equal(r.fallback_columns,0);assert.equal(r.independent_uncached_probe_matches,2);assert(r.max_scaled_error<1e-5);}
const step=tag=>{
  const r=read('joint-derivative-one-step-'+tag+'.result.json');
  assert.equal(r.search.history.length,1);
  assert.equal(r.search.history[0].inner.history.filter(h=>h.accepted).length,1);
  assert.equal(r.report.sampled_feasible,false);
  return {evaluations:r.search.evaluations,accepted_steps:1,speed_m_s:r.report.motion_report.speed_m_s,maximum_inequality:r.search.maximum_violation,inner_cost:r.search.history[0].inner.cost,force_derivative_mode:r.force_derivative_mode??'numerical'};
};
const multistart=['constant_mean-d0.5','constant_mean-d0.75'].map(group=>{
  const summary=read('joint-multistart-'+group+'.summary.json');
  const initial=read('joint-multistart-'+group+'-initial.result.json');
  const screened=read('joint-start-screen.summary.json').group_best.find(g=>g.group===group).best;
  assert.equal(initial.motion_report.maximum_force_error_n,screened.force_error_n);
  assert.equal(initial.motion_report.maximum_moment_error_nm,screened.moment_error_nm);
  assert.equal(initial.motion_report.minimum_torque_margin_nm,screened.torque_margin_nm);
  return {group,...summary};
});
const result={checks:{default_one_step_byte_identical:true,original_cad_report_identical:true,analytic_force_columns_checked:360,uncached_probe_matches:6,screen_initial_metrics_identical:true},derivative_cases:audit.cases,one_step:{numerical:step('new'),analytic:step('analytic')},completed_multistart:multistart,scope:'Derivative and local-solver evidence only. Both one-step results and both completed multistart results remain physically infeasible; no speed promotion or global maximum claim.'};
fs.writeFileSync(d+'joint-derivative-verification.json',JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({checks:result.checks,one_step:result.one_step}));
