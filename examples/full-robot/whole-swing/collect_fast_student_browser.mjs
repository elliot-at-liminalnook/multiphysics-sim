import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',run='runs/interactive/fast-distilled';
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const statusPath=`${root}/fast-student-browser-status.json`,status=read(statusPath);assert(status.complete);
const recordings={};
for(const c of status.cases){
  const r=c.measurement;assert(r.completed&&r.validation_passed&&r.keyboard_commands_recorded);assert.equal(r.frame_encoding,'json');assert.equal(r.display_rate_hz,0);
  assert.equal(r.runtime_build.browser_module_sha256,status.cases[0].measurement.runtime_build.browser_module_sha256);
  recordings[c.name]=read(`${run}/${c.name}.recording.json`);assert.equal(recordings[c.name].error,null);assert.equal(recordings[c.name].runtime.seed,0);
}
const a=recordings['previous-turn'],b=recordings['student-turn'];
assert.deepEqual(a.runtime.scene.robot,b.runtime.scene.robot);assert.deepEqual(a.runtime.scene.options,b.runtime.scene.options);assert.deepEqual(a.task,b.task);assert.deepEqual(a.runtime.input_events,b.runtime.input_events);
const physical=c=>{c=structuredClone(c);delete c.policy;return c;};assert.deepEqual(physical(a.runtime.config),physical(b.runtime.config));
const natives=[['previous-turn','runs/full-robot/learning/whole-exact-probe-base/exact-probe-base-enabled.native.json'],['student-turn','runs/full-robot/learning/fast-student-fidelity/turn-20ms.native.json'],['student-forward',`${run}/student-forward.native.json`]];
for(const [name,path] of natives)assert.deepEqual(read(path).recording,recordings[name].runtime);
const acceptancePath=`${run}/student-forward-acceptance/summary.json`,acceptance=read(acceptancePath);assert.equal(acceptance.capture.sha256,source(natives.at(-1)[1]).sha256);
const uiPath=`${run}/leaderboard.json`,ui=read(uiPath);assert(ui.passed);assert(status.parity.passed&&status.parity.replay_exact&&status.parity.reset_exact);
const result={version:1,passed:true,forward_acceptance:acceptance,ui,all_realtime_gates_passed:status.cases.filter(c=>c.name.startsWith('student-')).every(c=>c.measurement.meets_speed_target&&c.measurement.meets_transition_target),
  sources:[statusPath,acceptancePath,uiPath,...natives.map(([,p])=>p),...status.cases.map(c=>`${run}/${c.name}.recording.json`),import.meta.filename].map(source),
  scope:'Identical physical model, numerical options and held commands for previous/new rendered steering; controller recipe changes are explicit. Each exact browser recording matches the associated native capture. Forward acceptance retains its outcome independently of timing. All leaderboard loads, replay/video and integrity checks pass. Coarse accuracy, longer stopping and reserved transition failures remain disqualifying.'};
writeFileSync(`${root}/fast-student-browser-integrity.json`,JSON.stringify(result,null,2)+'\n');console.log({passed:true,forward_passed:acceptance.passed,all_realtime_gates_passed:result.all_realtime_gates_passed});
