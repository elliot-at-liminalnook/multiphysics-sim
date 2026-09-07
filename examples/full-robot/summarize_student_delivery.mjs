import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/interactive/student-robustness', read=p=>JSON.parse(readFileSync(p));
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const manifest=read(`${root}/viewer/build-manifest.json`);
for(const [p,h] of Object.entries(manifest.inputs))assert.equal(hash(p),h,p);
const parity=read(`${root}/parity.json`),ui=read(`${root}/viewer-report.json`),performance=read(`${root}/live-performance.json`);
assert(parity.passed&&ui.passed&&performance.completed&&performance.keyboard_commands_recorded);
const native=read('runs/full-robot/learning/student-robustness/validation-001/case-2.native.json');
const browser=read(`${root}/live-performance.recording.json`);
assert.deepEqual(browser.runtime.input_events,native.recording.input_events,'keyboard run must match the accepted native minute');
assert.deepEqual(browser.runtime.config,native.recording.config);
assert.deepEqual(browser.task,native.task);
const short=read(`${root}/viewer/data/robot-improved-student.json`);
assert.deepEqual(short.config,read('examples/full-robot/student-robustness/config.json'));
const validation=read('examples/full-robot/student-robustness/validation-status.json');
assert(validation.complete);
assert(validation.results.filter(r=>r.role==='development').every(r=>r.passed));
assert.equal(validation.results.find(r=>r.name==='heldout-selected').passed,false,'update documented limitation if evidence changes');
assert.equal(read('examples/full-robot/student-robustness/refined-5ms-status.json').passed,false);
const report={version:1,preset:'robot-improved-student',status:'Experimental improvement; not robust-controller promotion',
  learning:'Only neural weights changed; three development episodes pass independent checks.',
  limitations:['Reserved reverse-first push misses one swing.','Additional 5 ms refinement misses first swing.','Ideal actor observations and privileged upstream planner.','No hardware transfer, broad terrain, or command-to-visible latency acceptance.'],
  parity,ui,performance,keyboard_matches_native_minute:true,
  manifest_sha256:hash(`${root}/viewer/build-manifest.json`),
  sources:['examples/full-robot/student-robustness/search.recipe.json','examples/full-robot/student-robustness/validation.json',
    'examples/full-robot/student-robustness/heldout.config.json','examples/full-robot/student-robustness/refined-5ms.config.json',
    'examples/full-robot/student-robustness/policy.json','examples/full-robot/student-robustness/search-status.json',
    'examples/full-robot/student-robustness/validation-status.json','examples/full-robot/student-robustness/refined-5ms-status.json']
    .map(path=>({path,sha256:hash(path)}))};
writeFileSync('examples/full-robot/student-robustness/browser-status.json',JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({passed:true,speed:performance.meets_speed_target,transition:performance.meets_transition_target}));
