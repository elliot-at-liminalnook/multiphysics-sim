import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/interactive/heading-student',out='examples/full-robot/heading-task';
const read=p=>JSON.parse(readFileSync(p)),hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const validation=read(`${out}/validation-status.json`),search=read(`${out}/search-status.json`);
assert(validation.complete&&validation.passed&&validation.cases.length===5);
const ui=read(`${root}/viewer-report.json`),parity=read(`${root}/parity.json`),performance=read(`${root}/live-performance.json`);
assert(ui.passed&&parity.passed&&parity.replay_exact&&parity.reset_exact);
assert(performance.completed&&performance.keyboard_commands_recorded);
const manifest=read(`${root}/viewer/build-manifest.json`);
for(const [path,sha] of Object.entries(manifest.inputs))assert.equal(hash(path),sha,path);
const browser=read(`${root}/live-performance.recording.json`),native=read('runs/full-robot/learning/heading-task/selected-001/sustained.native.json');
assert.deepEqual(browser.runtime.config,native.recording.config);
assert.deepEqual(browser.runtime.input_events,native.recording.input_events);
assert.deepEqual(browser.task,native.task);
const report={version:1,preset:'robot-heading-student',status:'Experimental heading-aware student; five sampled walking checks pass; broader realtime/control goal remains incomplete',
 development_reward:{initial:search.initial_score,selected:search.best_score,fractional_improvement:search.best_score/search.initial_score-1},
 validation:validation.cases.map(c=>({name:c.name,reserved:c.reserved,passed:c.passed,swings:c.acceptance.lifts.length,
  final_heading_rad:c.final_heading.error_rad,final_body_error_m:c.acceptance.final_body_error_m})),
 parity,ui,performance,keyboard_matches_native_minute:true,
 manifest_sha256:hash(`${root}/viewer/build-manifest.json`),
 limitations:['Only 1.25 mm/s requested walking speed on a flat floor.','Ideal actor observations, no accumulated heading estimate and privileged planner.','Gentle reserved twists do not establish a broad recovery envelope.','No general command, terrain, command-to-visible-response or hardware-transfer acceptance.'],
 sources:[`${out}/search-status.json`,`${out}/validation-status.json`,`${out}/task.json`,`${out}/sustained.config.json`,
 'crates/sim-runtime/src/walking_task.rs','crates/sim-runtime/src/environment.rs','web/viewer/viewer.js'].map(path=>({path,sha256:hash(path)}))};
writeFileSync(`${out}/browser-status.json`,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({validated:true,speed:performance.meets_speed_target,transition:performance.meets_transition_target}));
