// Build typed demonstrations for an existing Rust residual network.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [descriptorPath]=process.argv.slice(2);assert(descriptorPath);
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const d=read(descriptorPath);assert.equal(d.version,1);
const pinned=s=>{assert.equal(source(s.path).sha256,s.sha256,`changed input: ${s.path}`);return read(s.path);};
for(const s of d.preparation_sources??[])assert.equal(source(s.path).sha256,s.sha256,`changed preparation source: ${s.path}`);
const network=pinned(d.network),scene=pinned(d.scene),config=pinned(d.config),task=pinned(d.task);
const loads=entries=>entries.map(e=>{
  const capture=pinned(e.capture);
  if(e.acceptance){const a=pinned(e.acceptance);assert(a.passed);assert.equal(a.capture.sha256,e.capture.sha256);}
  if(!capture.completed){assert.equal(e.allow_accepted_prefix,true);assert(capture.error&&capture.recording.completed_steps>0);}
  else assert(!capture.error);
  return {entry:e,capture};
});
const training=loads(d.training),validation=loads(d.validation);assert(training.length&&validation.length);
assert(training.every(x=>x.capture.completed&&x.entry.acceptance),'training requires accepted complete demonstrations');
for(const t of training)for(const v of validation){assert.notEqual(t.entry.capture.sha256,v.entry.capture.sha256);assert.notDeepEqual(t.capture.recording.input_events,v.capture.recording.input_events);}
const reference=training[0].capture;
const physicalConfig=c=>{c=structuredClone(c);for(const k of ['steps','report_every'])delete c[k];return c;};
for(const {capture:c} of [...training,...validation]){
  assert.deepEqual(c.recording.scene,reference.recording.scene);assert.deepEqual(c.task,task);
  assert.deepEqual(physicalConfig(c.recording.config),physicalConfig(reference.recording.config));
}
// The supplied student keeps the full authored CAD scene. Only its controller
// is changed; known fields absent from the Rust scene schema remain authored.
const omitted=new Set(pinned(d.scene_schema_omissions).fields);
function authored(a,b,path){
  if(a&&typeof a==='object'){
    assert(b&&typeof b==='object',path);if(Array.isArray(a))assert.equal(a.length,b.length,path);
    for(const key of Object.keys(a)){
      const p=`${path}.${key}`;
      if(!Object.hasOwn(b,key))assert(omitted.has(p.replace(/\.\d+/g,'.*')),`missing authored field: ${p}`);
      else authored(a[key],b[key],p);
    }
  }else assert.equal(a,b,path);
}
authored(scene.robot,reference.recording.scene.robot,'scene.robot');
authored(scene.options,reference.recording.scene.options,'scene.options');
assert.deepEqual(scene.controller.inputs,reference.recording.scene.controller.inputs);
const before=structuredClone(config);delete before.policy.neural_residual;
const recorded=structuredClone(reference.recording.config);delete recorded.policy.neural_residual;
authored(before,recorded,'config');
config.policy.neural_residual=network;
const contract=reference.metadata.policy_contract;
const sensors=[...new Map(network.features.flatMap(f=>[f.source,f.subtract].filter(Boolean).map(name=>{
  const c=contract.observations.find(c=>c.name===name);assert(c&&c.kind===f.kind,`typed feature: ${name}`);return [name,{name,kind:c.kind}];
}))).values()];
const actuators=network.outputs.map(o=>{const c=contract.actuators.find(c=>c.name===o.target);assert(c&&c.kind===o.kind);return {name:c.name,kind:c.kind};});
assert.deepEqual(d.baseline.map(b=>b.target),network.outputs.map(o=>o.target));
for(const b of d.baseline){for(const name of [b.reference,b.position])assert.equal(contract.observations.find(c=>c.name===name)?.kind,'Angle');assert.equal(contract.observations.find(c=>c.name===b.gain)?.kind,'Dimensionless');}
const inventory=[];
function dataset(entries,split){
  const samples=[];
  for(const {entry,capture} of entries){
    let previous=-Infinity,count=0;const maximum=network.outputs.map(()=>0);
    for(const frame of capture.frames){
      const p=frame.policy;if(!p)continue;
      assert(p.time_s>previous,'duplicate or out-of-order policy sample');previous=p.time_s;
      assert(p.time_s<frame.time_s,'labels must use observations held with the recorded action');
      const value=name=>{const x=p.observations[name];assert(Number.isFinite(x),name);return x;};
      const observations=sensors.map(s=>value(s.name));
      const actions=d.baseline.map((b,i)=>{
        const target=p.targets[b.target];assert(Number.isFinite(target));
        const actuatorIndex=contract.actuators.findIndex(c=>c.name===b.target);
        assert.equal(frame.servo_targets_rad[actuatorIndex],target,'policy target must equal the applied servo command');
        const residual=target-value(b.reference)-value(b.gain)*(value(b.reference)-value(b.position));
        maximum[i]=Math.max(maximum[i],Math.abs(residual));return residual;
      });
      samples.push({observations,actions});count++;
    }
    inventory.push({split,capture:entry.capture,samples:count,completed:capture.completed,error:capture.error,last_accepted_time_s:capture.frames.at(-1).time_s,maximum_absolute_residual_rad:maximum,
      outside_network_output_range:maximum.map((m,i)=>m>network.outputs[i].scale)});
  }
  return {version:1,sensors,actuators,samples};
}
const experiment={version:1,network,training:dataset(training,'training'),validation:dataset(validation,'validation'),optimizer:d.optimizer};
mkdirSync(d.output,{recursive:true});assert(!existsSync(`${d.output}/fit.json`),'refusing to replace an executed fit');
const write=(name,value)=>writeFileSync(`${d.output}/${name}.json`,JSON.stringify(value)+'\n');
write('initial.policy',network);write('scene',scene);write('initial.config',config);write('task',task);write('experiment',experiment);
write('manifest',{version:1,inputs:[source(descriptorPath),source(import.meta.filename),d.network,d.scene,d.config,d.task,d.scene_schema_omissions,...(d.preparation_sources??[]),...[...d.training,...d.validation].flatMap(e=>[e.capture,...(e.acceptance?[e.acceptance]:[])])],
  inventory,training_samples:experiment.training.samples.length,validation_samples:experiment.validation.samples.length,network_features:network.features,baseline:d.baseline,
  scope:'Rust fitting consumes only training samples and selects by training loss. Labels are accepted applied targets minus recorded reference/local tracking; no clipping or fabricated post-failure samples. Incomplete validation prefixes retain their failure and cannot qualify a full episode. Proposed encoder/IMU channels and upstream planning remain ideal simulation, not hardware deployable.'});
console.log({output:d.output,inventory});
