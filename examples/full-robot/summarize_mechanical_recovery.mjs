// Run after the four profiled step-margin recipes and the retained 5 ms repeat.
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/step-margin';
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const cases=[];
for(const name of ['retry-5ms-retained','retry-reverse','retry-sustained','retry-short']) {
  const path=`${root}/${name}.profile.json`, p=read(path), ds=p.accepted_intervals;
  assert(p.completed&&ds.length>0,'completed retained profile required: '+name);
  cases.push({name,profile_sha256:hash(path),accepted_nominal_intervals:ds.length,
    accepted_segments:p.accepted_implicit_steps.length,
    continuous_attempts:ds.reduce((s,d)=>s+d.continuous_attempts,0),
    rejected_trials:ds.reduce((s,d)=>s+d.rejected_trials,0),
    minimum_accepted_step_s:Math.min(...ds.map(d=>d.minimum_accepted_step_s)),
    recovered_intervals:ds.filter(d=>d.rejected_trials>0),
    maximum_accepted_newton_iterations:Math.max(...p.accepted_implicit_steps.map(d=>d.nonlinear.iterations))});
}
const a=read(`${root}/retry-5ms.native.json`),b=read(`${root}/retry-5ms-retained.diagnostic.json`);
// Hash normalized data instead of asking assertion formatting to print a huge
// failed object comparison. Host timings are deliberately outside replay state.
for(const c of [a,b])for(const frame of c.frames)delete frame.stepping_wall_s;
const stableHash=x=>createHash('sha256').update(JSON.stringify(x)).digest('hex');
assert.equal(stableHash(a.frames),stableHash(b.frames),'physical frame hash');
assert.equal(stableHash(a.transitions),stableHash(b.transitions),'task transition hash');
fs.writeFileSync('examples/full-robot/step-margin/recovery-status.json',JSON.stringify({
  version:1,
  scope:'Retained diagnostics of completed native intervals; failed final intervals and their retries are not included. Host-side history collection leaves all 1201 physical frames (excluding wall timing) and task transitions of the 5 ms run exactly unchanged. No performance acceptance claim from concurrently executed profiled runs. Initial profiles without retention were empty; these explicit diagnostic captures correct that gap.',
  cases,
},null,2)+'\n');
console.log(cases.map(c=>({name:c.name,retries:c.rejected_trials,min_step_s:c.minimum_accepted_step_s})));
