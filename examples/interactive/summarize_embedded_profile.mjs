// Summarize a complete profiling run, including rejected work and source identity.
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {cpus,platform,release} from 'node:os';
import assert from 'node:assert/strict';
const [profilePath,baselinePath,outputPath]=process.argv.slice(2);
assert(profilePath&&baselinePath&&outputPath,'usage: summarize_embedded_profile.mjs profile-capture.json baseline-capture.json output.json');
const inputs={};
async function read(path){const b=await readFile(path);inputs[path]=createHash('sha256').update(b).digest('hex');return JSON.parse(b);}
const p=await read(profilePath),b=await read(baselinePath);
assert(p.completed&&b.completed&&p.error===null&&b.error===null);
for(const field of ['source','scene_options','world','embedding','motor_experiment','policy_experiment','step_s','requested_steps'])assert.deepEqual(p[field],b[field]);
assert.deepEqual(p.frames,b.frames,'profiling must not alter recorded physical/control frames');
const sum=(rows,key)=>rows.reduce((n,r)=>n+r[key],0);
const counts={nominal_steps:p.hybrid_steps.length,accepted_segments:sum(p.hybrid_steps,'accepted_segments'),continuous_attempts:sum(p.hybrid_steps,'continuous_attempts'),
 minimum_accepted_step_s:Math.min(...p.hybrid_steps.map(s=>s.minimum_accepted_step_s)),
 successful_trial_endpoint_evaluations:sum(p.hybrid_solves,'successful_trial_endpoint_evaluations'),
 successful_trial_newton_iterations:sum(p.hybrid_solves,'successful_trial_newton_iterations'),
 successful_trials_with_reused_jacobian:sum(p.hybrid_solves,'successful_trials_with_reused_jacobian')};
assert(Array.isArray(p.solver_profile)&&p.solver_profile.some(v=>v.seconds>0));
const timing=p.solver_profile.map(v=>({...v,percentage_of_stepping_wall:100*v.seconds/p.stepping_wall_s,mean_microseconds_per_call:v.calls?1e6*v.seconds/v.calls:null}));
const report={hardware:{cpu:cpus()[0].model,logical_cpus:cpus().length,platform:platform(),os_release:release()},simulated_s:p.simulated_s,stepping_wall_s:p.stepping_wall_s,simulation_per_wall:p.simulated_s/p.stepping_wall_s,
 profiling_preserves_all_sampled_frames:true,counts,timing,
 scope:'Complete native development profile, including profiler overhead. Other development/browser processes may be running. Buckets overlap and nest; do not sum percentages. Successful-trial counters omit failed solver internals; continuous_attempts includes rejected hybrid work. A very small accepted segment can arise at an event boundary and does not itself prove convergence failure. No isolated performance or hardware-accuracy acceptance.',inputs};
await writeFile(outputPath,JSON.stringify(report,null,2));
console.log(JSON.stringify({simulated_s:report.simulated_s,wall_s:report.stepping_wall_s,counts,timing:timing.filter(v=>v.seconds>.5).map(v=>({name:v.name,seconds:v.seconds,calls:v.calls}))},null,2));
