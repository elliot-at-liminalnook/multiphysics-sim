// Independent contact schedules from exact policy replay; Rust owns geometry and forces.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
const [specPath,output]=process.argv.slice(2);
assert(specPath&&output&&!fs.existsSync(output),'usage: audit_contact_controller.mjs spec.json fresh-summary.json');
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const spec=read(specPath),prefix=spec.prefix;
const auditPrefix=spec.audit_prefix??prefix;
const capturePath=prefix+'.native.json',statePath=auditPrefix+'.policy-state.json';
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
const replayResult=spawnSync(bin+'/replay_policy_state',[capturePath],{encoding:'utf8',maxBuffer:64*1024*1024});
assert.equal(replayResult.status,0,replayResult.stderr);
if(fs.existsSync(statePath))assert.equal(fs.readFileSync(statePath,'utf8'),replayResult.stdout);
else fs.writeFileSync(statePath,replayResult.stdout,{flag:'wx'});
const capture=read(capturePath),replay=read(statePath),motion=capture.recording.scene.controller.parameters.motion;
assert(capture.completed&&replay.source_capture===capturePath&&replay.maximum_command_error_rad<=1e-12);
const markers=capture.recording.config.policy.task_observations.markers;
assert.equal(markers.length,motion.feet.length);
const planned=(state,leg)=>{
 const foot=motion.feet[leg],phase=((state.phase/motion.period_s-foot.phase_offset)%1+1)%1;
 return phase>=foot.stance_fraction;
};
const requirements=[],counts=[];
for(const [start,end] of spec.windows_s) {
 assert(start>=0&&end>start&&end<=capture.frames.at(-1).time_s);
 const samples=replay.samples.filter(s=>s.frame_time_s>=start-1e-9&&s.frame_time_s<=end+1e-9);
 assert(samples.length>2);
 for(let leg=0;leg<markers.length;leg++) {
  const intervals=[];let active=[];
  const flush=()=>{if(active.length&&active[0]>start+1e-9&&active.at(-1)<end-1e-9)intervals.push(active);active=[];};
  for(const s of samples){if(planned(s.state,leg))active.push(s.frame_time_s);else flush();}flush();
  assert(intervals.length>0,'each declared window must include complete swings for every foot');
  counts.push({window_s:[start,end],link:markers[leg].link,planned_swings:intervals.length});
  for(const times of intervals) requirements.push({start_s:times[0],end_s:times.at(-1),maximum_sample_gap_s:.021,
   qualifying_duration_s:.019999999,swing_link:markers[leg].link,minimum_clearance_m:.002,
   maximum_swing_force_n:1,support_check:'clearance_only',minimum_support_forces_n:{}});
 }
}
const reqPath=auditPrefix+'.contact-clearance-requirements.json',auditPath=auditPrefix+'.contact-geometry.json';
fs.writeFileSync(reqPath,JSON.stringify(requirements,null,2)+'\n',{flag:'wx'});
const out=fs.openSync(auditPath,'wx'),err=fs.openSync(auditPrefix+'.contact-geometry.log','wx');
const result=spawnSync(bin+'/evaluate_lift',[prefix+'.scene.json',capturePath,reqPath,'--simulation-time'],{stdio:['ignore',out,err]});
fs.closeSync(out);fs.closeSync(err);assert.equal(result.status,0,'Rust clearance/geometry audit failed');
const audit=read(auditPath);
const summary={prefix,audit_prefix:auditPrefix,counts,policy_replay_samples:replay.samples.length,maximum_command_error_rad:replay.maximum_command_error_rad,
 planned_foot_clearances:requirements.length,passed_foot_clearances:audit.reports.filter(r=>r.report.passed).length,
 failed_foot_clearances:audit.reports.filter(r=>!r.report.passed),inter_link_geometry_audit:audit.inter_link_geometry_audit,
 sources:[specPath,capturePath,statePath,reqPath,auditPath,import.meta.filename].map(path=>({path,sha256:hash(path)})),
 scope:'All complete independent planned swing intervals within explicit windows, reconstructed by exact Rhai replay. Shared Rust checks 2 mm clearance, <=1 N swing force and 20 ms qualifying duration. Clearance-only mode does not certify support or balance; closed-loop control and timestep tests are separate. Full CAD interlink geometry is checked at recorded poses; no between-sample guarantee.'};
fs.writeFileSync(output,JSON.stringify(summary,null,2)+'\n',{flag:'wx'});
console.log({planned:summary.planned_foot_clearances,passed:summary.passed_foot_clearances,geometry:summary.inter_link_geometry_audit});
