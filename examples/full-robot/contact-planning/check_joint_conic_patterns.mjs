import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import {gunzipSync} from 'node:zlib';
const d='examples/full-robot/contact-planning/',read=n=>JSON.parse(fs.readFileSync(d+n));
const same=(a,b,label)=>assert(isDeepStrictEqual(JSON.parse(JSON.stringify(a)),JSON.parse(JSON.stringify(b))),label);
assert(fs.readFileSync(d+'joint-conic-pattern-api-replay.result.json').equals(fs.readFileSync(d+'joint-conic-warm-final-rechecked.result.json')));
assert.equal(fs.readFileSync(d+'joint-conic-pattern-legacy.result.jsonl','utf8'),gunzipSync(fs.readFileSync(d+'joint-start-screen.result.jsonl.gz')).toString().split('\n')[0]+'\n');
const rows=fs.readFileSync(d+'joint-conic-pattern-pilot.result.jsonl','utf8').trim().split('\n').map(JSON.parse),batch=read('joint-conic-patterns.batch.json'),base=read('joint-timed-speed.recipe.json');
assert.equal(batch.starts.length,258);same(batch.robot,base.robot,'Batch changed physical recipe');
for(const [id,name] of [['control-fast-original-basis','joint-conic-fast.result.json'],['control-warm-feasible','joint-conic-warm-final-rechecked.result.json']]){
 const r=rows.find(r=>r.id===id).result,prior=read(name),m=prior.report.motion_report;
 same(r.candidate,prior.candidate,'Batch control changed optimized forces or motion');
 same(r.force_variables,base.variables.filter(v=>v.decision.kind==='force'),'Uniform bounds enumerator changed original force decisions');
 assert.equal(r.conic.minimax_balance,prior.search.reported_objective);assert.equal(r.conic.dual_objective,prior.search.reported_dual_objective);assert.equal(r.physical.sampled_feasible,prior.report.sampled_feasible);
 assert.equal(r.physical.force_error_n,m.maximum_force_error_n);assert.equal(r.physical.moment_error_nm,m.maximum_moment_error_nm);assert.equal(r.physical.torque_margin_nm,m.minimum_torque_margin_nm);
}
for(const start of batch.starts.slice(2)){
 const expected=structuredClone(base.candidate.motion),actual=start.candidate.motion;
 expected.feet.forEach((f,i)=>{f.phase_offset=actual.feet[i].phase_offset;f.stance_fraction=actual.feet[i].stance_fraction;});expected.body=actual.body;same(expected,actual,'Pattern changed properties beyond body reference and contact timing');
 assert.equal(start.candidate.force_timing,null);assert(start.align_and_bind_forces);
 for(const v of base.variables.filter(v=>v.decision.kind==='motion')){const q=v.decision.decision;let value;if(q.kind==='foot_phase')value=actual.feet[q.foot].phase_offset;else if(q.kind==='foot_stance')value=actual.feet[q.foot].stance_fraction;else if(q.kind==='body_control')value=actual.body.keyframes[q.control].values[q.channel];else continue;assert(value>=v.bound.lower&&value<=v.bound.upper,'Pattern left existing motion bounds');}
}
for(const row of rows){assert(!row.error);assert(row.result.independent_affine_error<1e-8);}
const overlap=JSON.parse(fs.readFileSync(d+'joint-conic-pattern-overlap.result.jsonl','utf8'));assert(!overlap.error);assert(overlap.result.independent_affine_error<1e-8);
const out={shared_api_replay_byte_identical:true,legacy_batch_first_row_byte_identical:true,known_fast_and_feasible_controls_reproduced:true,force_variable_enumerator_preserves_original_bounds_and_order:true,experimental_cases:256,all_trial_motion_changes_within_existing_bounds:true,pilot:rows.map(r=>({id:r.id,conic:r.result.conic,physical:r.result.physical})),overlap_probe:{id:overlap.id,conic:overlap.result.conic,physical:overlap.result.physical},scope:'Shared conic planner and systematic initialization screen verified against prior controls. New trial failures do not exclude optimized gait families; no runtime gain or physical speed maximum.'};
fs.writeFileSync(d+'joint-conic-pattern-verification.json',JSON.stringify(out,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({shared_api_byte_identical:true,legacy_byte_identical:true,controls_match:true,cases:258,pilot_cases:rows.length}));
