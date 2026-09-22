import fs from 'node:fs';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
const dir=import.meta.dirname,read=name=>JSON.parse(fs.readFileSync(`${dir}/${name}.json`));
const protocol=read('protocol');
function exact(a,b) {
 assert(a.completed&&b.completed&&!a.error&&!b.error);
 assert.equal(a.frames.length,b.frames.length);
 const fields=Object.keys(a.frames[0]).filter(k=>k!=='stepping_wall_s');
 for(let i=0;i<a.frames.length;i++) {
  assert.deepEqual(Object.keys(a.frames[i]),Object.keys(b.frames[i]));
  for(const key of fields)assert.deepEqual(a.frames[i][key],b.frames[i][key],`frame ${i}: ${key}`);
 }
 assert.deepEqual(a.transitions,b.transitions);
 return {frames:a.frames.length,exact_fields:fields,all_task_transitions_exact:true};
}
const report={scope:'Ordered native single-run measurements of one three-second command schedule; no repeated-machine estimate or timestep/hardware fidelity claim. Default lifetime remains 64. Original gates are unchanged.',default_parity:exact(read('before.native'),read('candidate.native')),runs:{}};
for(const folder of ['',...Object.keys(protocol.variants)]) {
 const prefix=folder?`${folder}/candidate`:'candidate';
 if(!fs.existsSync(`${dir}/${prefix}.profile.json`)) {report.runs[folder]={incomplete:true};continue;}
 const p=read(`${prefix}.profile`),capture=read(`${prefix}.native`);
 const parity=exact(capture,read(`${prefix}.profiled`));
 const stats={},maxima={};
 for(const row of p.motor_trial_statistics)for(const [k,v]of Object.entries(row)) {
  if(k.startsWith('maximum'))maxima[k]=Math.max(maxima[k]??0,v);else stats[k]=(stats[k]??0)+v;
 }
 const scheduler={attempts:0,accepted_segments:0,rejected_trials:0,events:0};
 for(const row of p.accepted_intervals) {scheduler.attempts+=row.continuous_attempts;scheduler.accepted_segments+=row.accepted_segments;scheduler.rejected_trials+=row.rejected_trials;scheduler.events+=row.events.length;}
 assert.equal(stats.successful_trials+scheduler.rejected_trials,scheduler.attempts);
 const qualified=spawnSync(process.execPath,[`${dir}/../qualify.mjs`,`jacobian-use-limit/${prefix}`,`jacobian-use-limit/${folder?'candidate':'before'}`],{encoding:'utf8'});
 assert.equal(qualified.status,0,qualified.stderr);
 const qualification=read(`${prefix}.qualification`);
 const inputs=folder?protocol.variants[folder].inputs:protocol.inputs;
 for(const [name,hash]of Object.entries(inputs))assert.equal(createHash('sha256').update(fs.readFileSync(`${dir}/${folder?folder+'/':''}${name}`)).digest('hex'),hash);
 assert.equal(read(`${prefix}.binary`).sha256,protocol.binary_candidate_sha256);
 report.runs[folder||'default']={limit:folder?protocol.variants[folder].step_jacobian_max_uses:64,qualification,profile_parity:parity,scheduler,statistics:stats,maxima,buckets:p.buckets.filter(r=>r.calls>0)};
 console.log(folder||'default',JSON.stringify({wall_s:capture.wall_s,speedup:qualification.speedup,accepted:qualification.accepted_as_solver_optimization,max_error:qualification.max_error,contacts:qualification.contactIdentities,reused:stats.successful_trials_with_reused_jacobian,outer_iterations:stats.successful_trial_newton_iterations,endpoints:stats.successful_trial_endpoint_evaluations,rejected_trials:scheduler.rejected_trials}));
}
const previous=read('../force-output-storage/candidate.profile');
const current=read('candidate.profile');
assert.deepEqual(previous.motor_trial_statistics,current.motor_trial_statistics);
assert.deepEqual(previous.accepted_intervals,current.accepted_intervals);
report.default_motor_and_interval_diagnostics_exact=true;
report.long_lifetime_physical_parity=exact(read('uses-1024/candidate.native'),read('uses-4096/candidate.native'));
assert.deepEqual(read('uses-1024/candidate.profile').motor_trial_statistics,read('uses-4096/candidate.profile').motor_trial_statistics);
assert.deepEqual(read('uses-1024/candidate.profile').accepted_intervals,read('uses-4096/candidate.profile').accepted_intervals);
report.long_lifetime_counters_and_intervals_exact=true;
const baseline=read('candidate.native');
report.controller_comparisons={};
for(const folder of Object.keys(protocol.variants)) {
 const candidate=read(`${folder}/candidate.native`);
 report.controller_comparisons[folder]=Object.fromEntries(['servo_states','servo_commands'].map(field=>[
  field,{equal:baseline.frames.every((frame,i)=>isDeepStrictEqual(frame[field],candidate.frames[i][field])),
    differing_frames:baseline.frames.filter((frame,i)=>!isDeepStrictEqual(frame[field],candidate.frames[i][field])).length}]));
}
fs.writeFileSync(`${dir}/analysis.json`,JSON.stringify(report,null,2)+'\n');
