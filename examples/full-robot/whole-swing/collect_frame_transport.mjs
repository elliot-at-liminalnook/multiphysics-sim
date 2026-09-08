import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',run='runs/interactive/frame-transport',read=p=>JSON.parse(readFileSync(p));
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const statusPath=`${root}/frame-transport-status.json`,status=read(statusPath);assert(status.complete);
const originalPath=`${root}/tangent-browser-status.json`,original=read(originalPath),cases=[];
for(const c of status.cases){
  const r=c.measurement;assert(r.completed&&r.validation_passed&&r.keyboard_commands_recorded);
  assert.equal(r.frame_encoding,c.name==='object-turn'?'object':'json');assert.equal(r.display_rate_hz,0);
  assert.equal(r.runtime_build.browser_module_sha256,original.episodes['live-turn'].runtime_build.browser_module_sha256);
  const recordPath=`${run}/${c.name}.recording.json`;
  const priorPath=`runs/interactive/portable-tangent/${c.name==='json-forward'?'live-forward':'live-turn'}.recording.json`;
  assert.deepEqual(read(recordPath),read(priorPath),'encoding must preserve the complete task recipe, seed and inputs');
  cases.push({name:c.name,frame_encoding:r.frame_encoding,active:r.performance.active_motion,overall_rate:r.performance.simulation_per_wall_second,
    meets_speed_target:r.meets_speed_target,meets_transition_target:r.meets_transition_target,sources:[recordPath,priorPath].map(source)});
}
const uiPath=`${run}/leaderboard.json`,fixturesPath=`${run}/fixtures-ui.json`,ui=read(uiPath),fixtures=read(fixturesPath);assert(ui.passed&&fixtures.passed);
const report={version:1,passed:true,cases,ui,fixtures,
  json_realtime_passed:cases.filter(c=>c.frame_encoding==='json').every(c=>c.meets_speed_target&&c.meets_transition_target),
  sources:[statusPath,originalPath,uiPath,fixturesPath,`${root}/FRAME-TRANSPORT-PLAN.md`,`${run}/fixtures/build-manifest.json`,import.meta.filename].map(source),
  scope:'Exact recording/recipe/input and WASM preservation links all cases to prior physically audited captures. Full JSON native/WASM parity, invalid encoding preservation, reset/replay, 11-entry UI, embedded/condensed fixture execution, load cancellation/recovery and error replay pass. Both JSON rendered p95 gates still fail. Encoding is a small measured cost, not a realtime or physical-accuracy qualification.'};
writeFileSync(`${root}/frame-transport-integrity.json`,JSON.stringify(report,null,2)+'\n');console.log({recordings_exact:true,json_realtime_passed:report.json_realtime_passed});
