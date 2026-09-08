import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',run='runs/interactive/portable-tangent';
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const selectionPath=`${root}/tangent-radius-selection.json`,selection=read(selectionPath);
const parityPath=`${root}/tangent-radius-browser-parity.json`,parity=read(parityPath).cases.find(c=>c.name===selection.selected.name).measurement;
const names=['reference-turn','live-turn','live-forward'];
const episodes=Object.fromEntries(names.map(n=>[n,read(`${run}/${n}.json`)]));
const recordings=Object.fromEntries(names.map(n=>[n,read(`${run}/${n}.recording.json`)]));
for(const name of names){
  assert(episodes[name].completed&&episodes[name].validation_passed&&episodes[name].keyboard_commands_recorded);
  assert.equal(recordings[name].error,null);assert.equal(recordings[name].runtime.seed,0);
  assert.equal(episodes[name].runtime_build.browser_module_sha256,episodes['live-turn'].runtime_build.browser_module_sha256);
}
const clean=r=>{r=structuredClone(r);delete r.runtime.config.implicit.linearized_jacobian_probes;delete r.runtime.config.implicit.linearized_probe_relative_step;return r;};
assert.deepEqual(clean(recordings['reference-turn']),clean(recordings['live-turn']),'same steering inputs and recipe except derivative probe options');
const nativePath=`runs/full-robot/learning/whole-tangent-radius/${selection.selected.name}.native.json`;
assert.deepEqual(read(nativePath).recording,recordings['live-turn'].runtime);
const forwardPath=`${run}/live-forward.native.json`,acceptancePath=`${run}/live-forward-acceptance/summary.json`;
assert.deepEqual(read(forwardPath).recording,recordings['live-forward'].runtime);
const acceptance=read(acceptancePath);assert.equal(acceptance.capture.sha256,source(forwardPath).sha256);
const uiPath=`${run}/leaderboard.json`,ui=read(uiPath);assert(ui.passed&&parity.passed&&parity.replay_exact&&parity.reset_exact);
const buildPath='runs/wasm-builds/tangent-radius-simd-lto/build.json';
const report={version:1,selection:selection.selected,build:read(buildPath),parity,ui,episodes,forward_acceptance:acceptance,
  all_realtime_gates_passed:['live-turn','live-forward'].every(n=>episodes[n].meets_speed_target&&episodes[n].meets_transition_target),
  sources:[selectionPath,parityPath,buildPath,nativePath,forwardPath,acceptancePath,uiPath,
    ...names.flatMap(n=>[`${run}/${n}.json`,`${run}/${n}.recording.json`,`${run}/${n}.timing.json`]),
    'runs/tangent-radius-live-turn.log',import.meta.filename,'web/tests/live_performance.mjs','web/tests/leaderboard.mjs'].map(source),
  scope:'Sequential rendered steering and forward/stop cases without concurrent heavy work. Same-binary reference steering differs only in derivative probes; native captures exactly match recorded recipes and controls. Fixed portability and exact replay/reset pass. Both candidate active p95 values exceed 20 ms; steering also misses overall realtime pace. The initial attempted config override was rejected by pinned-asset integrity before simulation (log retained). No held-out robustness, coarse timestep accuracy, sustained walking or hardware calibration claim.'};
writeFileSync(`${root}/tangent-browser-status.json`,JSON.stringify(report,null,2)+'\n');
console.log({forward_passed:acceptance.passed,all_realtime_gates_passed:report.all_realtime_gates_passed});
