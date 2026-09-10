// Diagnostics/evidence only: all residuals, optimization and physics are Rust.
import fs from 'node:fs';import assert from 'node:assert/strict';import {isDeepStrictEqual} from 'node:util';
const root='examples/full-robot/contact-implicit/';
const read=name=>JSON.parse(fs.readFileSync(root+name));
const diagnostic=read('stalled-descent.audit.json');
assert(isDeepStrictEqual(diagnostic,read('stalled-descent.replay.json')),'final-build diagnostic replay changed');
assert(isDeepStrictEqual(diagnostic.audits,read('stalled-derivatives.audit.json').audits),'initial coordinate audit changed');
const audits=diagnostic.audits.filter(a=>a.samples);
const g7=audits.map(a=>a.samples[3].residual_cost_slope),g9=audits.map(a=>a.samples[5].residual_cost_slope);
const dot=(a,b)=>a.reduce((s,v,i)=>s+v*b[i],0);
const result=read('resolved-work-finegrid-adaptive.result.json').stages.at(-1).result;
assert(isDeepStrictEqual(result.report,read('resolved-work-finegrid-adaptive.audit.json').planning),'uncached plan audit differs');
const earlier=read('resolved-work-runtime.summary.json').summaries.at(-1),current=read('adaptive-finegrid-runtime.summary.json').summaries.at(-1);
console.log(JSON.stringify({coordinate_count:audits.length,final_build_replay_exact:true,
  gradient_cosine_1e7_1e9:dot(g7,g9)/Math.sqrt(dot(g7,g7)*dot(g9,g9)),
  directions:diagnostic.descent_audits.map(a=>({gradient_probe_index:a.gradient_probe_index,
    predicted_cost_slope:a.predicted_directional_cost_slope,
    actual_small_step_slope:a.samples[4].residual_cost_slope,
    actual_forward_cost_slope:a.samples[4].forward_cost_slope,verification_step:a.samples[4].step})),
  adaptive_search:{refinements:result.search.derivative_refinements,termination:result.search.termination,
    initial_cost:result.search.initial_cost,final_cost:result.search.cost,
    derivative_transitions:result.search.history.filter((h,i,a)=>i===0||h.difference_step!==a[i-1].difference_step)
      .map(h=>({iteration:h.iteration,difference_step:h.difference_step})),
    within_planning_tolerances:result.report.within_planning_tolerances},
  matched_clock_runtime_comparison:{earlier:earlier.name,current:current.name,
    relative_mean_speed_increase:current.measured_mean_diagonal_body_speed_m_s/earlier.measured_mean_diagonal_body_speed_m_s-1,
    previous_speed_m_s:earlier.measured_mean_diagonal_body_speed_m_s,current_speed_m_s:current.measured_mean_diagonal_body_speed_m_s,
    previous_slip_ratio:earlier.maximum_loaded_material_slip_ratio,current_slip_ratio:current.maximum_loaded_material_slip_ratio,
    previous_joint_error_rad:earlier.maximum_joint_error_at_plan_knots_rad,current_joint_error_rad:current.maximum_joint_error_at_plan_knots_rad},
  scope:'Derivative failure and recovery, then short startup runtime measurement. Multiple planner changes contribute to the before/after comparison. No derivative certificate, sustained gait, realtime/WASD or physical maximum claim.'},null,2));
