import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',run='runs/interactive/display-cadence',read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const statusPath=`${root}/display-cadence-status.json`,status=read(statusPath);assert(status.complete);
const originalPath=`${root}/tangent-browser-status.json`,original=read(originalPath),cases=[];
for(const c of status.cases){
  const r=c.measurement;assert(r.completed&&r.validation_passed&&r.keyboard_commands_recorded);
  assert.equal(r.runtime_build.browser_module_sha256,original.episodes['live-turn'].runtime_build.browser_module_sha256);
  const recordPath=`${run}/${c.name}.recording.json`;
  const priorPath=`runs/interactive/portable-tangent/${c.name==='capped-forward'?'live-forward':'live-turn'}.recording.json`;
  assert.deepEqual(read(recordPath),read(priorPath),'display cadence must preserve the entire task recipe, seed and recorded inputs');
  cases.push({name:c.name,display_rate_hz:r.display_rate_hz,draws:r.performance.actual_drawn_frames,
    draws_per_wall_s:r.performance.actual_drawn_frames/r.performance.wall_s,active:r.performance.active_motion,
    overall_rate:r.performance.simulation_per_wall_second,meets_speed_target:r.meets_speed_target,meets_transition_target:r.meets_transition_target,
    sources:[recordPath,priorPath].map(source)});
}
const uiPath=`${run}/leaderboard.json`,ui=read(uiPath);assert(ui.passed);
const report={version:1,passed:true,cases,ui,
  capped_realtime_passed:cases.filter(c=>c.display_rate_hz===30).every(c=>c.meets_speed_target&&c.meets_transition_target),
  sources:[statusPath,originalPath,uiPath,`${root}/DISPLAY-CADENCE-PLAN.md`,import.meta.filename].map(source),
  scope:'Exact recording/recipe/input and compiled WASM preservation links every display case to prior physically audited captures and host parity. UI loads/replay/video and optional display control pass. Display cadence does not change Rust physics/control or simulation pacing. Timing results remain separate and retain all failures.'};
writeFileSync(`${root}/display-cadence-integrity.json`,JSON.stringify(report,null,2)+'\n');console.log({recordings_exact:true,capped_realtime_passed:report.capped_realtime_passed});
