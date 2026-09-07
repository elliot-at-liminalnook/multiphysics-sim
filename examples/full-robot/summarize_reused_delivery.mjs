import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/interactive/mechanical-restart',read=p=>JSON.parse(readFileSync(p));
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const manifest=read(`${root}/viewer/build-manifest.json`);
for(const [path,value] of Object.entries(manifest.inputs))assert.equal(hash(path),value,path);
const parity=read(`${root}/parity.json`),ui=read(`${root}/viewer-report.json`),performance=read(`${root}/live-performance.json`);
const finalMetadata=read(`${root}/final-metadata.json`);
assert(parity.passed&&ui.passed&&performance.completed&&performance.keyboard_commands_recorded);
assert(finalMetadata.passed&&finalMetadata.performance_limit_visible&&finalMetadata.heading_limit_visible);
const native=read('runs/full-robot/learning/mechanical-reuse/restart-sustained.native.json');
const browser=read(`${root}/live-performance.recording.json`);
assert.deepEqual(browser.runtime.input_events,native.recording.input_events);
assert.deepEqual(browser.runtime.config,native.recording.config);
assert.deepEqual(browser.task,native.task);
assert.deepEqual(read(`${root}/viewer/data/robot-reused-student.json`).config,read('examples/full-robot/mechanical-reuse/restart-sustained.config.json'));
const study=read('examples/full-robot/mechanical-reuse/study-status.json');
for(const name of ['restart-short','restart-sustained','restart-refined','restart-reverse'])assert(study.cases.find(c=>c.name===name)?.passed,name);
for(const name of ['restart-minute-push','refined-minute'])assert.equal(study.cases.find(c=>c.name===name)?.passed,false,'update heading limitations if evidence changes');
const comparison=read('examples/full-robot/mechanical-reuse/restart-sustained-comparison.json');
assert.equal(comparison.contact_pair_mismatches,0);assert.equal(comparison.phase_mismatches,0);
const report={version:1,preset:'robot-reused-student',status:'Experimental numerical optimization of the paced student; not general walking or hardware-transfer acceptance',
  changes:'Guarded cross-step derivative reuse, with a 24-correction first-attempt cap and fresh restart at the original 80-correction allowance before subdivision. Robot, policy weights, force laws and tolerances unchanged.',
  limitations:['Three-push 5 ms minute misses heading gate.','Unforced 5 ms reference also misses heading; coarse model remains timestep-sensitive.','Requested speed only 1.25 mm/s; ideal observations and privileged planner.','No broad terrain, hardware-transfer or command-to-visible-response acceptance.'],
  parity,ui,final_metadata:finalMetadata,performance,keyboard_matches_native_minute:true,comparison,
  manifest_sha256:hash(`${root}/viewer/build-manifest.json`),
  sources:['examples/full-robot/mechanical-reuse/study-status.json','examples/full-robot/mechanical-reuse/restart-sustained.config.json','examples/full-robot/mechanical-reuse/restart-short.config.json','crates/sim-domain-robot/src/articulated/embedding/mechanical_advance.rs','crates/sim-domain-robot/src/articulated/embedding/implicit.rs','crates/sim-runtime/src/embedded.rs'].map(path=>({path,sha256:hash(path)}))};
writeFileSync('examples/full-robot/mechanical-reuse/browser-status.json',JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({delivered:true,speed:performance.meets_speed_target,transition:performance.meets_transition_target}));
