import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const d='examples/full-robot/contact-planning/',read=n=>JSON.parse(fs.readFileSync(d+n));
const same=(a,b,label)=>assert(isDeepStrictEqual(JSON.parse(JSON.stringify(a)),JSON.parse(JSON.stringify(b))),label);
const r=read('joint-conic-mesh-refinement.result.json'),original=read('joint-conic-pilot-refinement.recipe.json'),fit=read('joint-conic-mesh-force.result.json');
same(r.coarse_report,read('joint-conic-feasible-pilot.result.json').report,'Rebuilt refinement changed original pilot physics');
const expected=structuredClone(original);expected.robot.additional_phases=r.refined_recipe.robot.additional_phases;same(expected,r.refined_recipe,'Refinement changed fields other than collocation phases');
for(const p of original.robot.additional_phases)assert(r.refined_recipe.robot.additional_phases.includes(p));
assert.equal(r.added_phases.length,8);assert.equal(r.coarse_report.motion_report.frames.length,250);assert.equal(r.audit_report.motion_report.frames.length,2250);assert.equal(r.refined_report.motion_report.frames.length,266);
const key=f=>JSON.stringify([f.clock.phase_rate,f.clock.phase_acceleration_per_s,f.time_s]);
const frames=new Map(r.refined_report.motion_report.frames.map(f=>[key(f),f]));
for(const f of r.coarse_report.motion_report.frames){const other=frames.get(key(f));assert(other,'Refinement lost a previous frame');const a={...f},b={...other};delete a.residual_weight;delete b.residual_weight;same(a,b,'Refinement changed old physical frame');}
assert.equal(Math.max(...r.refined_report.constraints.inequalities),Math.max(...r.audit_report.constraints.inequalities),'Refined mesh did not capture worst dense failure');
const period=original.candidate.motion.period_s;
for(const p of r.added_phases){const hits=r.refined_report.motion_report.frames.filter(f=>Math.abs(f.time_s/period-p)<1e-12);assert.equal(hits.length,2);assert(hits.every(f=>Math.max(...f.wrench_residual.map((x,i)=>Math.abs(x)/(i<3?original.robot.force_tolerance_n:original.robot.moment_tolerance_nm)))>1));}
same(fit.candidate.motion,original.candidate.motion,'Force refit changed motion');assert(fit.force_box_violation_n===0);assert(fit.report.maximum_cone_violation_n===0);assert(fit.report.motion_report.minimum_torque_margin_nm>0);assert(!fit.report.sampled_feasible);assert(fit.independent_affine_error<1e-8);
const pilot=read('joint-conic-mesh-pilot.result.json');same(pilot.initial_report,fit.report,'Refined native pilot initial report changed');same(pilot.search.final_evaluation.constraints,pilot.report.constraints.inequalities,'Native pilot differs from final full CAD audit');
const summarize=x=>({speed_m_s:x.motion_report.speed_m_s,force_error_n:x.motion_report.maximum_force_error_n,moment_error_nm:x.motion_report.maximum_moment_error_nm,torque_margin_nm:x.motion_report.minimum_torque_margin_nm,cone_violation_n:x.maximum_cone_violation_n,maximum_inequality:Math.max(...x.constraints.inequalities),sampled_feasible:x.sampled_feasible});
const out={added_phases:r.added_phases,old_physical_frames_retained:250,dense_frames:2250,refined_frames:266,only_collocation_phases_changed:true,worst_dense_failure_now_in_optimization_mesh:true,refined_initial:summarize(r.refined_report),force_refit:{native_status:fit.search.status,...summarize(fit.report)},native_pilot:{native_status:pilot.search.native_status,model_evaluations:pilot.model_evaluations,...summarize(pilot.report)},scope:'Adaptive collocation incorporates observed dense failures without changing physical limits or motion. No global speed optimum or measured runtime gain.'};
fs.writeFileSync(d+'joint-conic-mesh-verification.json',JSON.stringify(out,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(out));
