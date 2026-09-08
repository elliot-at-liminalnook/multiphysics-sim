import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',run='runs/interactive/exact-probe-base-measured-v2';
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const statusPath=`${root}/exact-probe-base-browser-status.json`,status=read(statusPath);assert(status.complete);
const recordings={};
for(const c of status.cases){
  const r=c.measurement;assert(r.completed&&r.validation_passed&&r.keyboard_commands_recorded);
  assert.equal(r.frame_encoding,'json');assert.equal(r.display_rate_hz,0);
  assert.equal(r.runtime_build.browser_module_sha256,status.cases[0].measurement.runtime_build.browser_module_sha256);
  recordings[c.name]=read(`${run}/${c.name}.recording.json`);
  assert.equal(recordings[c.name].error,null);assert.equal(recordings[c.name].runtime.seed,0);
}
const clean=r=>{r=structuredClone(r);delete r.runtime.config.implicit.reuse_exact_probe_base;return r;};
assert.deepEqual(clean(recordings['reference-turn']),clean(recordings['enabled-turn']));
const nativeSources=[];
for(const [name,path] of [['reference-turn','runs/full-robot/learning/whole-exact-probe-base/exact-probe-base-reference.native.json'],['enabled-turn','runs/full-robot/learning/whole-exact-probe-base/exact-probe-base-enabled.native.json'],['enabled-forward',`${run}/enabled-forward.native.json`]]){
  assert.deepEqual(read(path).recording,recordings[name].runtime);nativeSources.push(path);
}
const acceptancePath=`${run}/enabled-forward-acceptance/summary.json`,acceptance=read(acceptancePath);
assert(acceptance.passed);assert.equal(acceptance.capture.sha256,source(nativeSources.at(-1)).sha256);
const uiPath=`${run}/leaderboard.json`,ui=read(uiPath);assert(ui.passed);
const videoPath=`${run}/video-diagnostic.json`,video=read(videoPath);assert(video.passed);
assert(status.parity.passed&&status.parity.replay_exact&&status.parity.reset_exact);
const report={version:1,passed:true,forward_acceptance:acceptance,ui,video,
  ui_attempts:{initial:'Timed out waiting for a short video download. Focused 0.3/2 second exports and the full unchanged-duration retry passed; cause not reproduced.',diagnostic_events_added:true},
  all_realtime_gates_passed:status.cases.filter(c=>c.name.startsWith('enabled-')).every(c=>c.measurement.meets_speed_target&&c.measurement.meets_transition_target),
  sources:[statusPath,acceptancePath,uiPath,videoPath,'runs/exact-probe-base-ui.log','runs/exact-probe-base-ui-retry.log','web/tests/video-export.mjs',...nativeSources,...status.cases.map(c=>`${run}/${c.name}.recording.json`),import.meta.filename].map(source),
  scope:'Full exact recipe, seed and input identity connects rendered runs to independently accepted native captures. Same WASM across off/on measurements, fixed host parity, exact replay/reset and every leaderboard load pass. Timing outcomes are retained separately; no coarse timestep accuracy, held-out terrain or hardware qualification is implied.'};
writeFileSync(`${root}/exact-probe-base-browser-integrity.json`,JSON.stringify(report,null,2)+'\n');
console.log({passed:true,all_realtime_gates_passed:report.all_realtime_gates_passed});
