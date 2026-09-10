// Detect every unloaded interval using force alone, then independently test
// its CAD surface clearance. Count/gap gates prevent selecting isolated lifts.
import fs from 'node:fs';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
for(const name of process.argv.slice(2)){
 const def=read(`${d}/validation-cases.json`).rows.find(r=>r.name===name);if(!def)throw Error(`unknown case ${name}`);
 const c=read(`${def.prefix}.native.json`);if(!c.completed||c.error)throw Error('complete capture required');
 const feet=c.recording.config.policy.task_observations.markers.map(m=>m.link),indices=feet.map(n=>c.recording.scene.robot.links.findIndex(l=>l.name===n));
 const windows=def.kind==='human'?[[1.4,9.8],[11,15.8]]:def.kind==='sustained'?[[1.4,20],[25,40],[45,56]]:[[.8,3],[6.8,9]];
 const period=.8*.065/def.command_speed_m_s,requirements=[],counts=[];
 for(const [start,end] of windows)for(let leg=0;leg<4;leg++){
  const selected=c.frames.filter(f=>f.time_s>=start-1e-9&&f.time_s<=end+1e-9),intervals=[];let current=[];
  const flush=()=>{if(current.length>=2&&current[0]>start+1e-9&&current.at(-1)<end-1e-9)intervals.push(current);current=[];};
  for(const f of selected){
   const force=f.contacts.filter(p=>p.link===indices[leg]&&p.other==null).reduce((s,p)=>s+p.force_n[2],0);
   if(force<=1)current.push(f.time_s);else flush();
  }flush();
  const starts=intervals.map(w=>w[0]),gaps=starts.slice(1).map((t,i)=>t-starts[i]);
  const countMinimum=Math.max(1,Math.floor((end-start)/period)-1),maximumGap=gaps.length?Math.max(...gaps):null;
  const passed=intervals.length>=countMinimum&&(maximumGap===null||maximumGap<=1.2*period)&&starts[0]-start<=1.2*period&&end-(starts.at(-1)??start)<=1.4*period;
  counts.push({window_s:[start,end],link:feet[leg],detected_complete_unload_intervals:intervals.length,minimum_expected:countMinimum,maximum_start_gap_s:maximumGap,passed});
  for(const w of intervals)requirements.push({start_s:w[0],end_s:w.at(-1),maximum_sample_gap_s:.021,qualifying_duration_s:.019999999,
   swing_link:feet[leg],minimum_clearance_m:.002,maximum_swing_force_n:1,
   minimum_support_forces_n:Object.fromEntries((leg===0||leg===2?[1,3]:[0,2]).map(i=>[feet[i],1]))});
 }
 if(!requirements.length)throw Error('no complete periodic unloaded intervals');
 fs.writeFileSync(`${def.prefix}.lift-requirements.json`,JSON.stringify(requirements,null,2)+'\n');
 fs.writeFileSync(`${d}/${name}.lift-counts.json`,JSON.stringify({counts,passed:counts.every(c=>c.passed),scope:'All force-detected complete unload intervals in predeclared steady-command windows; cadence count/gap gates checked separately. Clearance was not used to select intervals.'},null,2)+'\n');
 const o=fs.openSync(`${def.prefix}.geometry.json`,'wx'),e=fs.openSync(`${def.prefix}.geometry-error.txt`,'wx');
 const r=spawnSync(`${bin}/evaluate_lift`,[`${def.prefix}.scene.json`,`${def.prefix}.native.json`,`${def.prefix}.lift-requirements.json`,'--simulation-time'],{stdio:['ignore',o,e],timeout:240000});fs.closeSync(o);fs.closeSync(e);
 if(r.status!==0)throw Error(`geometry audit failed ${name}: ${r.error?.message??r.status}`);
 const audit=read(`${def.prefix}.geometry.json`),summary={name,inter_link_geometry_audit:audit.inter_link_geometry_audit,
  periodic_lift_count_passed:counts.every(c=>c.passed),lift_windows:audit.reports.length,passed_lift_windows:audit.reports.filter(r=>r.report.passed).length,
  failed_lifts:audit.reports.filter(r=>!r.report.passed)};
 fs.writeFileSync(`${d}/${name}.geometry-summary.json`,JSON.stringify(summary,null,2)+'\n');console.log(summary);
}
