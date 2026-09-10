// Warm-start actuator learning without discarding an existing walking policy.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const [sourcePrefix,featurePolicy,output]=process.argv.slice(2);assert(output);
assert(!fs.existsSync(output));
const read=p=>JSON.parse(fs.readFileSync(p));
const source=path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const capture=read(sourcePrefix+'.native.json');assert(capture.completed&&capture.error===null);
const scene=structuredClone(capture.recording.scene),config=structuredClone(capture.recording.config);
assert(config.policy.neural_residual==null,'source must not already apply a network');
const network=read(featurePolicy),contract=capture.metadata.policy_contract;
for(const feature of network.features){
  for(const name of [feature.source,feature.subtract].filter(Boolean))
    assert.equal(contract.observations.filter(c=>c.name===name&&c.kind===feature.kind).length,1,'typed feature binding mismatch');
}
assert.equal(network.outputs.length,contract.actuators.length);
for(let i=0;i<network.outputs.length;i++){
  const a=contract.actuators[i],out=network.outputs[i],[lo,hi]=contract.software_target_bounds_rad[i];
  assert.equal(out.target,a.name);assert.equal(out.kind,a.kind);
  assert(Number.isFinite(lo)&&Number.isFinite(hi)&&lo<hi);
  // Any two valid commands differ by at most the complete command interval.
  // This scale therefore permits corrections spanning the entire actuator range.
  out.scale=hi-lo;
}
const last=network.layers.at(-1);assert(last.weights.length===network.outputs.length);
last.weights=last.weights.map(row=>row.map(()=>0));last.biases=last.biases.map(()=>0);
config.policy.neural_residual=network;config.policy.neural_command_saturation=true;
fs.mkdirSync(output,{recursive:true});
const write=(name,value)=>fs.writeFileSync(`${output}/${name}.json`,JSON.stringify(value)+'\n',{flag:'wx'});
write('zero.scene',scene);write('zero.config',config);write('zero.actions',read(sourcePrefix+'.actions.json'));
write('initial.policy',network);
write('manifest',{version:1,inputs:[source(sourcePrefix+'.native.json'),source(sourcePrefix+'.actions.json'),source(featurePolicy),source(import.meta.filename)],
  changes:['zero-output neural residual initialized with existing hidden features','explicit combined neural command saturation'],
  scope:'Retain the exact source Rhai gait and all CAD/world/actuator physics. Zero output must reproduce its physical trajectory. Learned corrections can span every actuator command interval; final requests are saturated by the shared Rust runtime. No tiny residual cap or gait-quality penalty. This initialization is not a trained speed improvement or a complete PLANC/PPO implementation.'});
console.log(output);
