import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual as eq} from 'node:util';
const d='examples/full-robot/contact-planning/',read=n=>JSON.parse(fs.readFileSync(d+n));
const r=read('joint-servo-command.recipe.json'),base=read('joint-body16-domain4000-optimization.recipe.json');
const c=JSON.parse(fs.readFileSync('runs/contact-planning/joint-body16-domain-screen.native.json'));
assert(eq(r.robot.independent_coordinates,c.metadata.coordinate_names));assert(eq(r.robot.servo_command_limits.bounds_rad,c.metadata.policy_contract.software_target_bounds_rad));
const old=structuredClone(r);delete old.robot.servo_command_limits;assert(eq(old,base),'Other model, motion or limits changed');
assert(fs.readFileSync(d+'joint-body16-domain-reference.result.json').equals(fs.readFileSync(d+'joint-servo-command-legacy-reference.result.json')),'Absent command limits changed legacy compilation');
const audit=read('joint-servo-command-jacobian.result.json'),other=read('joint-servo-command-quintic-jacobian.result.json'),report=audit.reference_report;
assert(eq(other.reference_report,report),'Selected audit cases changed the original report');
const cases=[...audit.cases,...other.cases];
assert.equal(report.motion_report.frames.length,274);assert.equal(report.constraints.inequalities.length,13976);
assert(cases.length===2&&cases.every(c=>c.max_scaled_error<=audit.scaled_error_tolerance&&c.independent_uncached_probe_matches===2));
const frames=report.motion_report.frames;assert(frames.every(f=>f.servo_command?.targets_rad.length===12&&f.servo_command.inequalities.length===24));
let worst=null;for(const f of frames)for(let j=0;j<12;j++){
 const target=f.servo_command.targets_rad[j],[lower,upper]=r.robot.servo_command_limits.bounds_rad[j];
 const violation=Math.max(0,target-upper,lower-target);
 assert(Math.abs(f.servo_command.inequalities[2*j]-(target-upper)/.01)<1e-12);
 assert(Math.abs(f.servo_command.inequalities[2*j+1]-(lower-target)/.01)<1e-12);
 if(worst===null||violation>worst.violation_rad)worst={time_s:f.time_s,phase:f.time_s/r.candidate.motion.period_s,clock:f.clock,coordinate:r.robot.independent_coordinates[j],target_rad:target,bounds_rad:[lower,upper],violation_rad:violation};
}
const summarize=p=>({speed_m_s:p.motion_report.speed_m_s,force_error_n:p.motion_report.maximum_force_error_n,moment_error_nm:p.motion_report.maximum_moment_error_nm,torque_margin_nm:p.motion_report.minimum_torque_margin_nm,cone_violation_n:p.maximum_cone_violation_n,servo_command_violation_rad:p.motion_report.frames.reduce((a,f)=>Math.max(a,f.servo_command?.maximum_violation_rad??0),0),maximum_inequality:p.constraints.inequalities.reduce((a,b)=>Math.max(a,b),0),sampled_feasible:p.sampled_feasible});
const pilot=read('joint-servo-command-pilot.result.json');assert(eq(pilot.initial_report,report),'Native initial report differs from independent audit');
if(pilot.search.final_evaluation)assert(eq(pilot.search.final_evaluation.constraints,pilot.report.constraints.inequalities),'Native constraints differ from full final audit');
const out={bounds_match_actual_runtime:true,only_explicit_command_limits_added:true,legacy_compiler_byte_identical:true,force_derivative_cases:cases,worst_nominal_command:worst,initial:summarize(report),native_pilot:{status:pilot.search.native_status,models:pilot.model_evaluations,model_budget_exhausted:pilot.model_budget_exhausted,native_final_constraints_available:pilot.search.final_evaluation!==null,native_final_constraints_equal:pilot.search.final_evaluation?true:null,grouped_body_probes:pilot.grouped_body_probes,body_group_fallbacks:pilot.body_group_fallbacks,...summarize(pilot.report)},scope:'Explicit nominal actuator-command limits are jointly differentiated and optimized. Runtime feedback/transition limits, full gait validation and greater measured speed remain unproven.'};
fs.writeFileSync(d+'joint-servo-command-verification.json',JSON.stringify(out,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(out));
