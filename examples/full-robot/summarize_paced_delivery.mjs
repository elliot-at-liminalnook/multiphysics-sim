import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/interactive/step-margin', read=p=>JSON.parse(readFileSync(p));
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const manifest=read(`${root}/viewer/build-manifest.json`);
for(const [p,h] of Object.entries(manifest.inputs))assert.equal(hash(p),h,p);
const parity=read(`${root}/parity.json`),recoveryParity=read(`${root}/recovery-parity.json`);
const ui=read(`${root}/viewer-report.json`),performance=read(`${root}/live-performance.json`);
const finalMetadata=read(`${root}/final-metadata.json`);
assert(parity.passed&&recoveryParity.passed&&ui.passed&&performance.completed&&performance.keyboard_commands_recorded);
assert(finalMetadata.passed&&finalMetadata.visible_performance_limit&&finalMetadata.visible_heading_limit);
const native=read('runs/full-robot/learning/step-margin/retry-sustained.native.json');
const browser=read(`${root}/live-performance.recording.json`);
assert.deepEqual(browser.runtime.input_events,native.recording.input_events,'keyboard inputs must match accepted native minute');
assert.deepEqual(browser.runtime.config,native.recording.config);
assert.deepEqual(browser.task,native.task);
const packaged=read(`${root}/viewer/data/robot-paced-student.json`);
assert.deepEqual(packaged.config,read('examples/full-robot/step-margin/retry-sustained.config.json'));
const study=read('examples/full-robot/step-margin/study-status.json');
for(const name of ['retry-short','retry-5ms','retry-diagonal','retry-reverse','retry-x','retry-sustained'])
  assert(study.cases.find(c=>c.name===name)?.passed,name);
assert.equal(study.cases.find(c=>c.name==='retry-minute')?.passed,false,'update heading limitation if evidence changes');
const report={version:1,preset:'robot-paced-student',status:'Experimental pacing and bounded solver recovery; not general walking or hardware-transfer acceptance',
  changes:'Student weights, task gates and physical force laws unchanged; adjusted planner support offsets and timing; opt-in shared implicit-step recovery.',
  limitations:['Three-push minute misses heading gate despite 26 supported swings.','Unforced minute heading is close to the acceptance limit.','Requested speed is only 1.25 mm/s.','Ideal observations and privileged planner.','No broad terrain, hardware-transfer or command-to-visible-response acceptance.'],
  parity,recovery_parity:recoveryParity,ui,final_metadata:finalMetadata,performance,keyboard_matches_native_minute:true,
  manifest_sha256:hash(`${root}/viewer/build-manifest.json`),
  sources:['examples/full-robot/step-margin/study-status.json','examples/full-robot/step-margin/recovery-status.json',
    'examples/full-robot/step-margin/retry-sustained.config.json','examples/full-robot/step-margin/retry-short.config.json',
    'crates/sim-domain-robot/src/articulated/embedding/mechanical_advance.rs','crates/sim-runtime/src/embedded.rs']
    .map(path=>({path,sha256:hash(path)}))};
writeFileSync('examples/full-robot/step-margin/browser-status.json',JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({delivered:true,speed:performance.meets_speed_target,transition:performance.meets_transition_target}));
