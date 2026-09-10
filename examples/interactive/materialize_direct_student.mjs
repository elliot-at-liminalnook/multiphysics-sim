// Replace a teacher motor policy with a fitted Rust network; retain its physics.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const [sourcePrefix,input,fit,output]=process.argv.slice(2);assert(output);
const read=p=>JSON.parse(fs.readFileSync(p));
const source=path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
assert(!fs.existsSync(output));
const capture=read(sourcePrefix+'.native.json');assert(capture.completed&&capture.error===null);
assert(isDeepStrictEqual(read(`${input}/experiment.json`),read(`${fit}/experiment.json`)));
const policy=read(`${fit}/policy.json`),result=read(`${fit}/fit.json`);
assert(isDeepStrictEqual(policy,result.policy));
const baseline=read(`${input}/baseline.json`),contract=capture.metadata.policy_contract;
assert.equal(policy.outputs.length,contract.actuators.length);
for(let i=0;i<contract.actuators.length;i++){
  const a=contract.actuators[i],[lo,hi]=contract.software_target_bounds_rad[i],o=policy.outputs[i];
  assert.equal(o.target,a.name);assert.equal(o.kind,a.kind);
  assert.equal(baseline[a.name],(lo+hi)/2);assert.equal(o.scale,(hi-lo)/2);
}
const scene=structuredClone(capture.recording.scene),config=structuredClone(capture.recording.config);
scene.controller.parameters={baseline};
scene.controller.sources={entry:'direct_student.rhai',files:{'direct_student.rhai':
  'fn control(t, sensors, commands, state) { let p = parameters(); for name in commands.keys() { commands[name] = p.baseline[name]; } #{commands: commands, state: state} }'}};
config.policy.neural_residual=policy;
// These produced teacher suggestions, never physical forces. The student uses
// only its declared observations and has no access to those suggestions.
delete config.policy.body_feedback;delete config.policy.point_feedback;delete config.policy.step_reference;
config.policy.feedback_observations=false;
fs.mkdirSync(output,{recursive:true});
const write=(name,value)=>fs.writeFileSync(`${output}/${name}.json`,JSON.stringify(value)+'\n',{flag:'wx'});
write('student.scene',scene);write('student.config',config);write('student.actions',read(sourcePrefix+'.actions.json'));
write('manifest',{version:1,inputs:[source(sourcePrefix+'.native.json'),source(sourcePrefix+'.actions.json'),
  ...['experiment','baseline'].map(n=>source(`${input}/${n}.json`)),...['policy','fit','validation'].map(n=>source(`${fit}/${n}.json`)),source(import.meta.filename)],
  changed:['controller parameters and program','policy neural_residual','policy body_feedback/point_feedback/step_reference removed','policy feedback_observations false'],
  scope:'Direct neural actuator commands spanning the recorded command bounds. Identical normalized CAD robot, world, integrator, task conditions and source input sequence. No reference gait or teacher feedback is consumed by the motor policy. Sensor channels remain ideal simulation observations.'});
console.log(output);
