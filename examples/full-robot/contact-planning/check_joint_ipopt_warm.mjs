import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning/';
const read=name=>JSON.parse(fs.readFileSync(d+name));
const same=(a,b,message)=>assert(isDeepStrictEqual(JSON.parse(JSON.stringify(a)),JSON.parse(JSON.stringify(b))),message);
const old=read('joint-timed-speed.result.json'), ordinary=read('joint-ipopt-warm-one-step.result.json'), close=read('joint-ipopt-warm-close-one-step.result.json');
for(const [name,result] of [['default',ordinary],['close',close]]) {
 same(result.initial_report,old.report,`${name}: transferred initial full report differs`);
 assert.equal(result.returned_candidate_error,null);
 assert.equal(result.model_evaluations,316);
 same(result.search.final_evaluation.constraints,result.report.constraints.inequalities,`${name}: native constraints differ from uncached audit`);
 assert.equal(result.search.final_evaluation.objective,.5*result.report.constraints.objective[0]**2);
 assert.equal(result.report.constraints.inequalities.length,6776);
}
const native=read('joint-ipopt-warm-native-audit.result.json'),previous=read('joint-ipopt-interface-final.result.json');
for(const row of previous.cases) same(native.cases.find(x=>x.case===row.case),row,`Prior native case changed: ${row.case}`);
const distances=native.cases.filter(x=>x.case==='initial_bound_distance');
assert.equal(distances.length,2);
for(const row of distances) assert(Math.abs(row.result.values[0]-row.push)<1e-14);
const summarize=r=>({native_status:r.search.native_status,model_evaluations:r.model_evaluations,initial:r.initial_report.motion_report.speed_m_s,initial_force_error_n:r.initial_report.motion_report.maximum_force_error_n,final_speed_m_s:r.report.motion_report.speed_m_s,force_error_n:r.report.motion_report.maximum_force_error_n,moment_error_nm:r.report.motion_report.maximum_moment_error_nm,torque_margin_nm:r.report.motion_report.minimum_torque_margin_nm,maximum_inequality:Math.max(...r.report.constraints.inequalities),feasible:r.report.sampled_feasible,iterations:r.search.iterations});
const identity=name=>{const path=d+name,bytes=fs.readFileSync(path);return {path,bytes:bytes.length,sha256:crypto.createHash('sha256').update(bytes).digest('hex')};};
const result={initial_full_reports_identical:true,native_final_equals_independent_full_cad:true,unchanged_native_regression_cases:previous.cases.length,initial_distance_cases:distances.length,default_initialization:summarize(ordinary),close_initialization:summarize(close),artifacts:['joint-ipopt-warm-one-step.result.json','joint-ipopt-warm-close-one-step.result.json','joint-ipopt-warm-native-audit.result.json','check_joint_ipopt_warm.mjs'].map(identity),scope:'Primal initialization comparison at the same .30 m/s target, physical limits, variables and bounds. Neither pilot establishes convergence, runtime speed or a global physical maximum.'};
fs.writeFileSync(d+'joint-ipopt-warm-verification.json',JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({...result,artifacts:undefined}));
