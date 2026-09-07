import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/interactive/demand-render',out='examples/full-robot/browser-precision';
const read=p=>JSON.parse(readFileSync(p)),hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const validation=read(`${out}/validation-status.json`),screen=read(`${out}/guarded.screen.json`);
assert(validation.complete&&validation.passed&&screen.passed);
const minuteWalking=read('runs/full-robot/learning/heading-performance/guarded-acceptance/summary.json');
assert(minuteWalking.passed);
const ui=read(`${root}/viewer-report.json`),parity=read(`${root}/parity.json`);
assert(ui.passed&&parity.passed&&parity.replay_exact&&parity.reset_exact);
const manifest=read(`${root}/viewer/build-manifest.json`);
for(const [path,value] of Object.entries(manifest.inputs))assert.equal(hash(path),value,path);
const episodes=[
 ['sustained','live-performance','runs/full-robot/learning/heading-performance/guarded.native.json'],
 ['turn-reverse','turn-performance','runs/full-robot/learning/heading-performance/guarded-validation-001/turn-reverse.native.json'],
].map(([name,file,nativePath])=>{
 const performance=read(`${root}/${file}.json`),record=read(`${root}/${file}.recording.json`),native=read(nativePath);
 assert(performance.completed&&performance.keyboard_commands_recorded);
 assert(performance.performance.actual_drawn_frames>0,'actual drawing must be measured');
 assert.deepEqual(record.runtime.config,native.recording.config);
 assert.deepEqual(record.runtime.scene,native.recording.scene);
 assert.deepEqual(record.runtime.input_events,native.recording.input_events);
 assert.deepEqual(record.task,native.task);
 return {name,report:performance,keyboard_matches_native:true,native_capture_sha256:hash(nativePath),recording_sha256:hash(`${root}/${file}.recording.json`)};
});
const report={version:1,preset:'robot-browser-solver',status:'Experimental browser improvement; straight-minute p95 still exceeds target',
 numerical_screen:screen,minute_walking:minuteWalking,validation:validation.cases.map(c=>({name:c.name,passed:c.passed,swings:c.acceptance.lifts.length,
  final_heading_rad:c.final_heading.error_rad,final_body_error_m:c.acceptance.final_body_error_m})),
 parity,ui,episodes,manifest_sha256:hash(`${root}/viewer/build-manifest.json`),
 limitations:['Requested walking speed remains only 1.25 mm/s.','Ideal observations and privileged planner; no deployable sensing or general terrain acceptance.','Minute-long p95 remains above 20 ms despite realtime average speed.','Conditional drawing preserves physics updates but may combine multiple completed states into the next display frame.','rAF timing and draw counts do not measure actual display presentation or command-to-visible-motion latency.'],
 sources:[`${out}/guarded.config.json`,`${out}/validation-status.json`,`${out}/plan.json`,
 'web/viewer/viewer.js','web/tests/live_performance.mjs','crates/sim-runtime/src/environment.rs'].map(path=>({path,sha256:hash(path)}))};
writeFileSync(`${out}/browser-status.json`,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({validated:true,episodes:episodes.map(e=>({name:e.name,speed:e.report.meets_speed_target,transition:e.report.meets_transition_target}))}));
