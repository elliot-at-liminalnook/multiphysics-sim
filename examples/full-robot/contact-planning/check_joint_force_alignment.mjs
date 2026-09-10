import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const d='examples/full-robot/contact-planning/',read=n=>JSON.parse(fs.readFileSync(d+n));
const r=read('joint-force-alignment-checked.result.json'),initial=read('joint-force-alignment.result.json');
for(const key of ['recipe','original_report','resampled_report','resampled_force_templates','report'])assert(isDeepStrictEqual(r[key],initial[key]),'Final helper changed '+key);
let maximumCommonDifference=0,matched=0;
for(const old of r.original_report.motion_report.frames){
  const next=r.resampled_report.motion_report.frames.find(x=>x.clock.phase_rate===old.clock.phase_rate&&x.clock.phase_acceleration_per_s===old.clock.phase_acceleration_per_s&&Math.abs(x.time_s-old.time_s)<1e-12);assert(next);matched++;
  for(let i=0;i<6;i++)maximumCommonDifference=Math.max(maximumCommonDifference,Math.abs(old.wrench_residual[i]-next.wrench_residual[i])/(i<3?r.recipe.robot.force_tolerance_n:r.recipe.robot.moment_tolerance_nm));
  old.torque_capacity_margin_nm.forEach((m,i)=>maximumCommonDifference=Math.max(maximumCommonDifference,Math.abs(m-next.torque_capacity_margin_nm[i])/r.recipe.robot.torque_tolerance_nm));
  for(const key of ['maximum_inter_link_penetration_m','maximum_floor_penetration_m'])maximumCommonDifference=Math.max(maximumCommonDifference,Math.abs(old[key]-next[key])/r.recipe.robot.penetration_tolerance_m);
}
assert.equal(matched,168);assert(maximumCommonDifference<1e-6);
assert.equal(r.linear_templates_checked,8);assert(r.maximum_linear_force_curve_change_n<1e-10);
const derivative=read('joint-aligned-force-jacobian.result.json');
assert(isDeepStrictEqual(JSON.parse(JSON.stringify(derivative.reference_report)),read('joint-aligned-speed-initial.result.json')),'Independent aligned initial report mismatch');
assert.deepEqual(derivative.cases.map(c=>c.case),['shifted_reference','constant_body','alternate_interpolation']);
for(const c of derivative.cases){assert.equal(c.checked_force_columns,366);assert.equal(c.fallback_columns,0);assert.equal(c.independent_uncached_probe_matches,2);assert(c.max_scaled_error<1e-5);}
const log=fs.readFileSync(d+'joint-force-alignment-final-tests.log','utf8');
assert(log.includes('5 passed; 0 failed'));
const bound=Number(log.match(/seven-node linear normalized residual lower bound ([\d.e+-]+)/)[1]);
assert(bound>.001);
const summary={original_linear_templates:8,maximum_preserved_force_curve_error_n:r.maximum_linear_force_curve_change_n,matched_original_frames:matched,maximum_normalized_common_frame_difference:maximumCommonDifference,helper_reports_reproduced:true,independent_initial_report_equal_after_signed_zero_canonicalization:true,distinct_derivative_configurations:3,checked_force_columns:1098,independent_uncached_probe_matches:6,maximum_derivative_scaled_error:Math.max(...derivative.cases.map(c=>c.max_scaled_error)),isolated_two_support_fixture:{original_seven_node_linear_residual_lower_bound:bound,aligned_linear_maximum_error:Number(log.match(/event-aligned linear maximum error ([\d.e+-]+)/)[1]),duty_fraction:.65,phase_offsets:[0,.5],required_total_force:1,scope:'Total vertical load only, with no geometry, actuator or robot-speed certificate. Floating-point affine lower bound on original node values in [-2,2].'},scope:'Representation and derivative fidelity. The prepared CAD candidate remains infeasible; no new measured speed or global physical limit is established.'};
fs.writeFileSync(d+'joint-force-alignment-verification.json',JSON.stringify(summary,null,2)+'\n');
console.log(JSON.stringify(summary));
