// Extract recorded observation/action pairs for the shared Rust imitation fitter.
// This prepares artifacts only; inference, optimization and physics stay in Rust.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const [descriptorPath]=process.argv.slice(2);assert(descriptorPath);
const read=p=>JSON.parse(fs.readFileSync(p));
const source=path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const d=read(descriptorPath);assert.equal(d.version,1);
assert(d.training.length&&d.validation.length&&new Set(d.features).size===d.features.length);
assert(!fs.existsSync(d.output),'output must be fresh');
let contract=null,bounds=null;const inventory=[],ranges=[];
function dataset(entries,split){
  const samples=[];
  for(const e of entries){
    assert.equal(source(e.capture.path).sha256,e.capture.sha256);
    const c=read(e.capture.path);assert(c.completed&&c.error===null,'complete demonstrations required');
    const cc=c.metadata.policy_contract;
    if(!contract){contract=cc;bounds=cc.software_target_bounds_rad;}
    assert(isDeepStrictEqual(cc.actuators,contract.actuators));
    assert(isDeepStrictEqual(cc.software_target_bounds_rad,bounds));
    let labels=null;
    if(e.labels){
      assert.equal(source(e.labels.path).sha256,e.labels.sha256,'changed teacher labels');
      assert.equal(e.labels.capture_sha256,e.capture.sha256,'labels belong to another capture');
      labels=read(e.labels.path);
      assert.equal(labels.source_capture,e.capture.path,'label source path mismatch');
      assert(isDeepStrictEqual(labels.actuators.map(({name,kind})=>({name,kind})),cc.actuators.map(({name,kind})=>({name,kind}))), 'teacher label actuator order/units differ');
      assert.equal(labels.samples.length,c.frames.filter(f=>f.policy).length,'one teacher label per recorded decision required');
    }
    for(const name of d.features){
      const a=contract.observations.filter(o=>o.name===name),b=cc.observations.filter(o=>o.name===name);
      assert(a.length===1&&isDeepStrictEqual(a,b),`typed observation: ${name}`);
    }
    const start=e.start_s??0,end=e.end_s??c.recording.scene.duration_s;
    assert(start>=0&&end>start&&end<=c.recording.scene.duration_s);
    for(const r of ranges)if(r.sha256===e.capture.sha256&&r.split!==split)
      assert(end<=r.start||start>=r.end,'training/validation intervals overlap');
    ranges.push({sha256:e.capture.sha256,start,end,split});
    let previous=-Infinity,count=0,labelIndex=0;
    for(const f of c.frames){
      const p=f.policy;if(!p)continue;
      assert(p.time_s>previous&&p.time_s<f.time_s,'use the observations recorded with the applied action');
      previous=p.time_s;
      const label=labels?.samples[labelIndex++];
      if(label){assert.equal(label.policy_time_s,p.time_s,'teacher label time mismatch');assert.equal(label.targets_rad.length,contract.actuators.length);}
      if(p.time_s<start||p.time_s>=end)continue;
      const observations=d.features.map(name=>{const v=p.observations[name];assert(Number.isFinite(v));return v;});
      const actions=contract.actuators.map((a,i)=>{
        assert.equal(p.targets[a.name],f.servo_targets_rad[i],'recorded command differs from applied command');
        const target=label?label.targets_rad[i]:p.targets[a.name],[lo,hi]=bounds[i];
        assert(Number.isFinite(target)&&target>=lo&&target<=hi);
        return target-(lo+hi)/2;
      });
      samples.push({observations,actions});count++;
    }
    assert(count>0);inventory.push({split,...e,samples:count,start_s:start,end_s:end});
  }
  return {version:1,sensors:d.features.map(name=>{const c=contract.observations.find(o=>o.name===name);return {name,kind:c.kind};}),
    actuators:contract.actuators.map(({name,kind})=>({name,kind})),samples};
}
const training=dataset(d.training,'training'),validation=dataset(d.validation,'validation');
let features=training.sensors.map((s,i)=>{
  const center=training.samples.reduce((sum,x)=>sum+x.observations[i],0)/training.samples.length;
  const sd=Math.sqrt(training.samples.reduce((sum,x)=>sum+(x.observations[i]-center)**2,0)/training.samples.length);
  return {source:s.name,subtract:null,kind:s.kind,center,scale:sd>1e-6?sd:1,clip:1e6};
});
let seed=d.seed>>>0;
function random(){seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed/4294967296;}
let width=features.length;
const layers=[...d.hidden_widths,contract.actuators.length].map(next=>{
  assert(Number.isInteger(next)&&next>0);const amplitude=Math.sqrt(6/(width+next));
  const layer={weights:Array.from({length:next},()=>Array.from({length:width},()=>amplitude*(2*random()-1))),biases:Array(next).fill(0)};
  width=next;return layer;
});
let network={version:1,features,outputs:contract.actuators.map((a,i)=>({target:a.name,kind:a.kind,scale:(bounds[i][1]-bounds[i][0])/2})),layers};
if(d.initial_policy){
  assert.equal(source(d.initial_policy.path).sha256,d.initial_policy.sha256,'changed warm-start policy');
  const initial=read(d.initial_policy.path);
  assert(isDeepStrictEqual(initial.outputs,network.outputs),'warm-start command ranges differ');
  assert(isDeepStrictEqual(initial.features.map(f=>[f.source,f.kind,f.subtract]),features.map(f=>[f.source,f.kind,f.subtract])),'warm-start feature bindings differ');
  // Existing weights are meaningful only with their original normalization.
  network=initial;features=initial.features;
}
const baseline=Object.fromEntries(contract.actuators.map((a,i)=>[a.name,(bounds[i][0]+bounds[i][1])/2]));
fs.mkdirSync(d.output,{recursive:true});
const write=(name,value)=>fs.writeFileSync(`${d.output}/${name}.json`,JSON.stringify(value)+'\n',{flag:'wx'});
write('experiment',{version:1,network,training,validation,optimizer:d.optimizer});
write('baseline',baseline);write('initial.policy',network);
write('manifest',{version:1,inputs:[source(descriptorPath),source(import.meta.filename),...inventory.flatMap(e=>[e.capture,...(e.labels?[e.labels]:[])]),...(d.initial_policy?[d.initial_policy]:[])],inventory,
  training_samples:training.samples.length,validation_samples:validation.samples.length,baseline,features,
  scope:'Direct actuator policy represented as CAD/software command midpoint plus a Rust tanh network spanning the full recorded command interval. Optional counterfactual teacher labels replace training targets only, never the recorded actions or physics. Normalization uses training data or the pinned warm-start artifact. Validation may be temporally held out from the same trajectory and is not an independent robustness trial. No runtime physics, learner or neural inference is implemented by this extractor.'});
console.log(JSON.stringify({output:d.output,training_samples:training.samples.length,validation_samples:validation.samples.length,features:features.length}));
