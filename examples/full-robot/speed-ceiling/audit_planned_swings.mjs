// Planned swings come only from verified replay of the original Rhai controller.
// Contact loss outside those windows is retained as a separate diagnostic.
import fs from 'node:fs';
import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
for(const name of process.argv.slice(2)) {
 const def=read(`${d}/validation-cases.json`).rows.find(r=>r.name===name);if(!def)throw Error(`unknown ${name}`);
 const prefix=def.prefix,c=read(`${prefix}.native.json`);
 const statePath=`${prefix}.policy-state.json`;
 {
  const r=spawnSync(`${bin}/replay_policy_state`,[`${prefix}.native.json`],{encoding:'utf8',maxBuffer:32*1024*1024});
  if(r.status!==0)throw Error(r.stderr);
  if(fs.existsSync(statePath)){if(fs.readFileSync(statePath,'utf8')!==r.stdout)throw Error('cached replay differs');}
  else fs.writeFileSync(statePath,r.stdout);
 }
 const replay=read(statePath),p=c.recording.scene.controller.parameters;
 if(replay.source_capture!==`${prefix}.native.json`||replay.maximum_command_error_rad>1e-12)throw Error('verified matching replay required');
 const feet=c.recording.config.policy.task_observations.markers.map(m=>m.link);
 const windows=def.kind==='human'?[[1.4,9.8],[11,15.8]]:def.kind==='sustained'?[[1.4,20],[25,40],[45,56]]:[[.8,3],[6.8,9]];
 const requirements=[],counts=[],incidental=[],groups=[];
 const planned=(s,leg)=>{const u=s.phase%(p.period_s/2);return u>p.swing_start_s&&u<p.swing_end_s&&((s.phase<p.period_s/2)===(leg===0||leg===2));};
 for(const [start,end] of windows)for(let leg=0;leg<4;leg++) {
  const samples=replay.samples.filter(s=>s.frame_time_s>=start-1e-9&&s.frame_time_s<=end+1e-9);
  const intervals=[];let active=[];
  const flush=()=>{if(active.length&&active[0]>start+1e-9&&active.at(-1)<end-1e-9)intervals.push(active);active=[];};
  for(const s of samples){if(planned(s.state,leg))active.push(s.frame_time_s);else flush();}flush();
  const minimum=Math.max(1,Math.floor((end-start)/(p.period_s*p.nominal_speed_m_s/def.command_speed_m_s))-1);
  counts.push({window_s:[start,end],link:feet[leg],planned_swings:intervals.length,minimum_expected:minimum,passed:intervals.length>=minimum});
  for(const times of intervals)requirements.push({start_s:times[0],end_s:times.at(-1),maximum_sample_gap_s:.021,qualifying_duration_s:.019999999,
   swing_link:feet[leg],minimum_clearance_m:.002,maximum_swing_force_n:1,
   minimum_support_forces_n:Object.fromEntries((leg===0||leg===2?[1,3]:[0,2]).map(i=>[feet[i],1]))});
  groups.push({samples,leg});
 }
 const reqPath=`${prefix}.planned-lift-requirements.json`,auditPath=`${prefix}.planned-geometry.json`;
 fs.writeFileSync(reqPath,JSON.stringify(requirements,null,2)+'\n');
 if(fs.existsSync(auditPath))throw Error(`refusing overwrite ${auditPath}`);
 const out=fs.openSync(auditPath,'wx'),err=fs.openSync(`${prefix}.planned-geometry-error.txt`,'wx');
 const r=spawnSync(`${bin}/evaluate_lift`,[`${prefix}.scene.json`,`${prefix}.native.json`,reqPath,'--simulation-time'],{stdio:['ignore',out,err],timeout:240000});fs.closeSync(out);fs.closeSync(err);
 if(r.status!==0)throw Error(`audit failed ${r.status}`);
 const a=read(auditPath);
 for(const {samples,leg} of groups){
  const geometry=new Map(a.sample_series[feet[leg]].map(s=>[s.time_s,s]));let unload=[];
  const saveUnload=()=>{if(unload.length)incidental.push({link:feet[leg],start_s:unload[0].time_s,end_s:unload.at(-1).time_s,samples:unload.length,
   maximum_clearance_m:Math.max(...unload.map(s=>s.swing_clearance_m))});unload=[];};
  for(const s of samples){const g=geometry.get(s.frame_time_s);if(!g)throw Error('missing geometric sample');
   if(!planned(s.state,leg)&&g.floor_forces_n[feet[leg]]<=1)unload.push(g);else saveUnload();
  }saveUnload();
 }
 const summary={name,policy_replay_samples:replay.samples.length,maximum_command_error_rad:replay.maximum_command_error_rad,
  counts,planned_lifts:requirements.length,passed_planned_lifts:a.reports.filter(r=>r.report.passed).length,
  failed_planned_lifts:a.reports.filter(r=>!r.report.passed),inter_link_geometry_audit:a.inter_link_geometry_audit,
  incidental_stance_unloads:incidental,scope:'All complete planned swing windows from exact Rhai state replay in declared steady-command windows. Stance unloads are reported separately, including boundary delays; no between-sample guarantee.'};
 fs.writeFileSync(`${d}/${name}.planned-geometry-summary.json`,JSON.stringify(summary,null,2)+'\n');
 console.log({name,planned:summary.planned_lifts,passed:summary.passed_planned_lifts,incidental:incidental.length});
}
