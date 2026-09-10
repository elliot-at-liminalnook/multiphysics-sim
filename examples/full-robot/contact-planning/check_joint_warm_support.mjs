import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning/';
const read=n=>JSON.parse(fs.readFileSync(d+n));
const same=(a,b,label)=>assert(isDeepStrictEqual(JSON.parse(JSON.stringify(a)),JSON.parse(JSON.stringify(b))),label);
const identity=n=>{const path=d+n,bytes=fs.readFileSync(path);return {path,bytes:bytes.length,sha256:crypto.createHash('sha256').update(bytes).digest('hex')};};
const checks=[];
for(const name of ['warm','body16']) {
 const r=read(`joint-support-${name}-contract.result.json`), prior=read(`joint-support-${name}-box.result.json`);
 for(const key of ['bound','box_probes','fitted_candidate','fitted_physical_report','independent_affine_error']) same(r[key],prior[key],`Contract audit changed ${name} ${key}`);
 assert.equal(r.columns,366); assert.equal(r.rows,name==='warm'?1500:1596);
 assert.equal(r.bounded_validation_tolerance,1e-8);
 assert.equal(r.box_probes.length,5);
 for(const p of r.box_probes) {assert(Number.isFinite(p.maximum_normalized_affine_error));assert(p.maximum_normalized_affine_error<1e-8);assert(p.maximum_measured_residual+1e-8>=r.bound.maximum_residual_lower_bound);}
 assert.equal(r.least_squares_inside_variable_box,false);
 assert(r.independent_affine_error>1e-8);
 assert(r.maximum_absolute_least_squares_force_n>1e9);
 assert.equal(r.contract_check.selected_reordered_columns,183);
 assert(r.contract_check.maximum_normalized_affine_error<1e-8);
 assert(r.contract_check.empty_duplicate_out_of_range_rejected);
 assert(r.contract_check.motion_decision_present_and_rejected);
 const instantaneous=read(name==='warm'?'joint-warm-support-instantaneous.result.json':'joint-support-body16-instantaneous.result.json');
 assert.equal(instantaneous.frames_proven_outside_tolerance,0);
 assert.equal(instantaneous.independent_reconstruction_error,0);
 assert(instantaneous.maximum_residual_lower_bound<1);
 checks.push({name,rows:r.rows,columns:r.columns,whole_curve_lower_bound:r.bound.maximum_residual_lower_bound,instantaneous_lower_bound:instantaneous.maximum_residual_lower_bound,maximum_bounded_probe_error:Math.max(...r.box_probes.map(p=>p.maximum_normalized_affine_error)),contract:r.contract_check,out_of_box_least_squares_error:r.independent_affine_error});
}
const original=read('joint-ipopt-warm.recipe.json'), recipe=read('joint-warm-force-only.recipe.json'), result=read('joint-warm-force-only.result.json');
same(recipe.candidate,original.candidate,'Force-only starting motion or forces changed');
same(recipe.robot,original.robot,'Physical model changed');
same(recipe.search,original.search,'Recipe search settings changed');
same(recipe.variables,original.variables.filter(v=>v.decision.kind==='force'),'Force-only variables or bounds changed');
same(result.candidate.motion,original.candidate.motion,'Force-only optimizer changed motion');
same(result.initial_report,read('joint-timed-speed.result.json').report,'Force-only initial CAD report changed');
const projected=new Set([...result.constraint_projection.fixed_zero_rows,...result.constraint_projection.collision_domain_rows]);
for(const i of projected) assert(result.report.constraints.inequalities[i]===0);
same(result.search.final_evaluation.constraints,result.report.constraints.inequalities.filter((_,i)=>!projected.has(i)),'Native constraints differ from final independent CAD audit');
assert.equal(result.search.native_status,-1);assert.equal(result.model_evaluations,276);assert.equal(result.model_budget_exhausted,false);assert.equal(result.report.sampled_feasible,false);assert.equal(result.best_sampled_feasible,null);
for(const n of ['joint-warm-support-basis','joint-warm-support-analytic']) {assert.equal(fs.statSync(d+n+'.result.json').size,0);assert(fs.readFileSync(d+n+'.log','utf8').includes('affine relaxation disagrees'));}
const out={checks,force_only:{physical_model_and_motion_preserved:true,native_constraints_match_full_audit:true,model_evaluations:276,native_status:-1,sampled_feasible:false},scope:'Bounded affine verification and failed conditional force-only solve. Bounds below one neither prove nor exclude feasibility. No runtime speed gain or physical maximum.',artifacts:['joint-support-warm-contract.result.json','joint-support-body16-contract.result.json','joint-warm-force-only.result.json','check_joint_warm_support.mjs'].map(identity)};
fs.writeFileSync(d+'joint-warm-support-verification.json',JSON.stringify(out,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({...out,artifacts:undefined}));
