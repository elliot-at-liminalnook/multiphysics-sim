// Inspect captured outcomes, not a second implementation of the controller.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const [capturePath,outputPath]=process.argv.slice(2);
assert(capturePath&&outputPath,'usage: audit_landing_checkpoint.mjs capture.json audit.json');
const bytes=await readFile(capturePath),capture=JSON.parse(bytes);
assert(capture.completed===true&&capture.error===null,'experiment must complete');
const gate=capture.motion_gate, trace=capture.motion_gate_trace;
assert(gate&&Array.isArray(trace)&&trace.length,'support trace required');
const waiting=trace.filter(s=>!s.clock.advancing&&!s.clock.timed_out&&s.clock.reference_tick*gate.clock.period_s<gate.clock.duration_s-1e-10);
const resumes=trace.filter((s,i)=>i>0&&s.clock.advancing&&!trace[i-1].clock.advancing);
const terminal=capture.terminal_frame.motion_progress;
assert(terminal?.phase==='complete','reference must finish');
assert(waiting.length&&resumes.length,'experiment must exercise a pause and resume');
assert(!trace.some(s=>s.clock.timed_out),'unexpected timeout');
// Check sustained observations preceding each actual resume, including samples
// before the guard: qualification is a moving observation window.
const qualifications=resumes.map(s=>{
 const start=s.time_s-gate.clock.qualification_s;
 const window=trace.filter(x=>x.time_s>=start-1e-10&&x.time_s<=s.time_s+1e-10);
 assert(window.length===Math.round(gate.clock.qualification_s/gate.clock.period_s)+1,'qualification coverage missing');
 assert(window.every(x=>x.upward_forces_n.length===gate.support_links.length&&x.upward_forces_n.every(f=>Number.isFinite(f)&&f>=gate.minimum_upward_force_n)),'resume without sustained support');
 return {resume_s:s.time_s,qualified_from_s:window[0].time_s,minimum_foot_force_n:Math.min(...window.flatMap(x=>x.upward_forces_n))};
});
const closure={};
for(const frame of capture.frames)for(const row of frame.original_rows){
 const key=row.unit;
 closure[key]??={maximum_position:0,maximum_velocity:0,maximum_acceleration:0};
 for(const field of ['position','velocity','acceleration']){
  assert(Number.isFinite(row[field]),'nonfinite closure');
  closure[key]['maximum_'+field]=Math.max(closure[key]['maximum_'+field],Math.abs(row[field]));
 }
}
assert(Array.isArray(capture.contact_steps)&&capture.contact_steps.length,'accepted contact history required');
let internal=0,maximumPenetration=0;
for(const step of capture.contact_steps)for(const c of step.contacts){
 if(c.other!==null)internal++;
 assert(Number.isFinite(c.penetration_m),'nonfinite penetration');
 maximumPenetration=Math.max(maximumPenetration,c.penetration_m);
}
const report={passed:internal===0,capture_sha256:createHash('sha256').update(bytes).digest('hex'),simulated_s:capture.simulated_s,wall_s:capture.stepping_wall_s,step_s:capture.step_s,
 reference_completed_at_s:trace.find(s=>s.clock.reference_tick*gate.clock.period_s>=gate.clock.duration_s-1e-10)?.time_s,
 waiting_sample_count:waiting.length,waiting_s:waiting.length*gate.clock.period_s,first_wait_s:waiting[0].time_s,qualifications,
 maximum_sampled_original_closure:closure,accepted_internal_contact_samples:internal,maximum_accepted_penetration_m:maximumPenetration,
 scope:'Support checkpoint execution and original-closure diagnostics. Sampled forces qualify dwell; not continuous-force proof, landing placement, balance, calibrated contact or walking acceptance.'};
await writeFile(outputPath,JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));assert(report.passed,'unexpected internal contact');
