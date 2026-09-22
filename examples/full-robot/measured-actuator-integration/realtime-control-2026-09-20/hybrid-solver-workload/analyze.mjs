import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const root=import.meta.dirname;
const read=name=>JSON.parse(fs.readFileSync(`${root}/${name}`));
const a=read('../contiguous-joint-axes/candidate.native.json');
const b=read('candidate.profiled.json'), p=read('candidate.profile.json');
assert(a.completed && b.completed && p.completed && !a.error && !b.error);
assert.equal(a.frames.length,b.frames.length);
const fields=Object.keys(a.frames[0]).filter(k=>k!=='stepping_wall_s');
for(let i=0;i<a.frames.length;i++) {
 assert.deepEqual(Object.keys(a.frames[i]),Object.keys(b.frames[i]));
 for(const key of fields) assert.deepEqual(a.frames[i][key],b.frames[i][key],`frame ${i} ${key}`);
}
assert.deepEqual(a.transitions,b.transitions);
assert.equal(p.motor_trial_statistics.length,p.accepted_intervals.length);
const totals={}, maxima={};
for(const row of p.motor_trial_statistics) for(const [k,v] of Object.entries(row)) {
 if(k.startsWith('maximum_')) maxima[k]=Math.max(maxima[k]??0,v);
 else {assert(Number.isSafeInteger(v)&&v>=0,k);totals[k]=(totals[k]??0)+v;}
}
const scheduler={accepted_segments:0,continuous_attempts:0,rejected_trials:0,events:0};
for(const row of p.accepted_intervals) {
 for(const k of ['accepted_segments','continuous_attempts','rejected_trials'])scheduler[k]+=row[k];
 scheduler.events+=row.events.length;
}
assert.equal(totals.successful_trials+scheduler.rejected_trials,scheduler.continuous_attempts);
const identity=read('protocol.json');
for(const [name,hash] of Object.entries(identity.inputs))assert.equal(createHash('sha256').update(fs.readFileSync(`${root}/${name}`)).digest('hex'),hash);
assert.equal(read('candidate.binary.json').sha256,identity.binary_candidate_sha256);
const report={scope:p.scope,physical_parity:{frames:a.frames.length,exact_fields:fields,all_task_transitions_exact:true},outer_intervals:p.accepted_intervals.length,scheduler,totals,maxima,per_successful_trial:Object.fromEntries(Object.entries(totals).map(([k,v])=>[k,v/totals.successful_trials])),profile_wall_s:p.wall_s,profile_buckets:p.buckets.filter(b=>b.calls>0),input_and_binary_identity_verified:true};
fs.writeFileSync(`${root}/analysis.json`,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({...report,profile_buckets:undefined},null,2));
