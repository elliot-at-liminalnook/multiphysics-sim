// Check original-input provenance independently of the replay/parity assertions.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
const [root]=process.argv.slice(2);
assert(root,'usage: check_robot_input_evidence.mjs evidence-directory');
const read=file=>JSON.parse(fs.readFileSync(path.join(root,file)));
const reports=[];
for(const robot of ['wheeled','quadruped']){
 for(const candidate of ['original','edited']){
  const key=`${robot}-${candidate}`;
  const spec=read(`${key}-spec.json`),native=read(`${key}-native.json`);
  const browser=read(`${key}-browser.json`),capture=read(`${key}-browser-capture.json`);
  assert(native.passed&&browser.passed,`${key}: runtime acceptance`);
  assert(browser.robot_input_checked&&browser.robot_inspection_checked,`${key}: inspection coverage`);
  assert.equal(browser.robot_input_override_count,candidate==='edited'?1:0);
  assert.deepEqual(native.full.recording.runtime.scene.robot,spec.scene.robot,`${key}: recorded input fields`);
  assert.deepEqual(native.full.recording.runtime.scene.robot_input,spec.scene.robot_input,`${key}: recorded receipt`);
  assert.deepEqual(capture.robotInput,native.robot_input,`${key}: exact input binding`);
  assert.deepEqual(capture.robotInspection,native.robot_inspection,`${key}: exact inspection`);
  assert.equal(native.robot_input.origin,'episode_document');
  if(candidate==='edited'){
   const binding=native.robot_input;
   assert(binding.input.defaulted_fields.includes('/world/ambient_c'));
   assert(binding.input.unmodeled_fields.includes('/cad_input_fixture'));
   assert.deepEqual(binding.overrides,[{pointer:'/world/ambient_c',original:{presence:'absent'},value:21}]);
   assert.equal(spec.scene.robot.world.ambient_c,21);
   assert.deepEqual(native.robot_inspection.robot_input,spec.scene.robot_input);
   assert(browser.invalid_receipts_preserved);
  }
  reports.push({robot,candidate,runtime:native.experiment.runtime,passed:true,
   overrides:native.robot_input.overrides.length,maximum_native_wasm_difference:browser.maximum_native_wasm_difference});
 }
}
assert(reports.every(r=>JSON.stringify(r.runtime)===JSON.stringify(reports[0].runtime)),'same runtime');
const report={version:1,passed:true,reports,scope:'Preserved input fields and overrides, exact metadata and same-host checkpoint replay, numeric native/WASM trajectory parity. Short synthetic fixtures; no locomotion, physical calibration or realtime qualification.'};
fs.writeFileSync(path.join(root,'acceptance.json'),JSON.stringify(report,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify(report,null,2));
