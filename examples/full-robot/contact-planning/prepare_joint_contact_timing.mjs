import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import crypto from 'node:crypto';
const dir='examples/full-robot/contact-planning/';
const source=dir+'joint-height-speed.recipe.json';
const artifact=dir+'joint-contact-timing.result.json';
const original=JSON.parse(fs.readFileSync(source)), r=JSON.parse(fs.readFileSync(artifact));
const strip=x=>{const y=structuredClone(x);delete y.candidate.force_timing;delete y.candidate.force_templates;return y;};
assert(isDeepStrictEqual(strip(original),strip(r.recipe)),'Non-force recipe changed');
assert.equal(r.recipe.variables.length,443);
assert.equal(r.stale_resolved_cached_and_fixed_reports_identical,true);
assert(r.maximum_curve_error_n<1e-10);
assert(isDeepStrictEqual(r.original_report,JSON.parse(fs.readFileSync(dir+'joint-height-speed-initial.result.json'))),'Legacy full CAD report changed');
for(let c=0;c<original.candidate.force_templates.length;c++) for(let f=0;f<original.candidate.force_templates[c].length;f++) {
  const a=original.candidate.force_templates[c][f],b=r.recipe.candidate.force_templates[c][f];
  assert.equal(a.interpolation,b.interpolation);assert.equal(a.keyframes.length,b.keyframes.length);
  a.keyframes.forEach((k,i)=>assert(isDeepStrictEqual(k.values,b.keyframes[i].values),'Force coefficients changed'));
}
const summary=x=>({speed_m_s:x.motion_report.speed_m_s,frames:x.motion_report.frames.length,force_error_n:x.motion_report.maximum_force_error_n,moment_error_nm:x.motion_report.maximum_moment_error_nm,torque_margin_nm:x.motion_report.minimum_torque_margin_nm,cone_violation_n:x.maximum_cone_violation_n,maximum_inequality:Math.max(0,...x.constraints.inequalities),sampled_feasible:x.sampled_feasible});
const outputs=[
  ['joint-timed-speed.recipe.json',r.recipe],
  ['joint-timed-speed-initial.result.json',r.bound_report],
  ['joint-timed-phase-probe.recipe.json',r.probe_recipe],
];
for(const [p,data] of outputs) fs.writeFileSync(dir+p,JSON.stringify(data,null,2)+'\n',{flag:'wx'});
const identity=path=>({path,bytes:fs.statSync(path).size,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const result={inputs:[source,artifact].map(identity),outputs:outputs.map(([p])=>identity(dir+p)),original:summary(r.original_report),bound:summary(r.bound_report),timing_probe:summary(r.probe_report),maximum_curve_error_n:r.maximum_curve_error_n,maximum_knot_time_error:r.maximum_knot_time_error,event_order:r.recipe.candidate.force_timing.event_order,legacy_full_report_identical:true,stale_resolved_cached_and_fixed_reports_identical:true,scope:'Contact-relative force knot timing only; coefficients, physical model, 443 variables and their bounds, objective and search settings unchanged. Strict initial event ordering adds an explicit local domain restriction. Extracted JSON canonicalizes signed zeros.'};
fs.writeFileSync(dir+'joint-contact-timing-preparation.json',JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify(result));
