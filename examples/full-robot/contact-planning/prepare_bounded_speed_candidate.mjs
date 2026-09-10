import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
const [source,prefix,durationArg='20']=process.argv.slice(2);
assert(source&&prefix);
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
const rawScene=read(source+'.scene.json'),rawConfig=read(source+'.config.json'),capture=read(source+'.native.json');
// Rust's recording contains the actual validated schema/defaults used by the
// source run. CAD export-only metadata need not survive serde round-tripping.
const scene=structuredClone(capture.recording.scene),config=structuredClone(capture.recording.config);
const actions=read(source+'.actions.json'),contract=capture.metadata.policy_contract;
assert(isDeepStrictEqual(scene.controller,rawScene.controller),'recorded controller differs from source input');
assert.equal(config.step_s,rawConfig.step_s);
assert.equal(config.motors.expected_cad_sha256,rawConfig.motors.expected_cad_sha256);
assert(isDeepStrictEqual(config.policy.target_bounds_rad,rawConfig.policy.target_bounds_rad),'recorded command bounds differ');
assert(config.policy.neural_residual==null);
const bounds=contract.software_target_bounds_rad,actuators=contract.actuators;
assert(bounds.length===actuators.length&&bounds.every(b=>b.length===2&&b.every(Number.isFinite)&&b[0]<=b[1]));
scene.controller.parameters.output_bounds=Object.fromEntries(actuators.map((ch,i)=>{
  assert(ch.unit==='rad');return [ch.name,bounds[i]];
}));
const sources=scene.controller.sources,program=sources.files[sources.entry];
assert.equal((program.match(/fn control\(/g)??[]).length,1);
const wrapper='examples/interactive/bounded_commands.rhai';
sources.files[sources.entry]=program.replace('fn control(','fn unbounded_control(')+'\n'+fs.readFileSync(wrapper,'utf8');
const duration=Number(durationArg);assert(Number.isFinite(duration)&&duration>0);
scene.duration_s=duration;config.steps=Math.round(duration/config.step_s);
const packet=scene.controller.inputs.findIndex(ch=>ch.name==='command.packet_sequence');assert(packet>=0);
assert(actions.every(row=>row.every((v,i)=>i===packet||v===actions[0][i])));
const schedule=Array.from({length:Math.round(duration/scene.period_s)},(_,i)=>actions[0].map((v,j)=>j===packet?i+1:v));
for(const [suffix,value] of [['scene',scene],['config',config],['actions',schedule]])write(prefix+'.'+suffix+'.json',value);
write(prefix+'.preparation.json',{source,inputs:['scene','config','actions','native'].map(s=>({path:source+'.'+s+'.json',sha256:hash(source+'.'+s+'.json')})),
  wrapper:{path:wrapper,sha256:hash(wrapper)},code:{path:import.meta.filename,sha256:hash(import.meta.filename)},
  output_bounds:scene.controller.parameters.output_bounds,
  scope:'Explicit policy output saturation using the exact software/CAD intersection exported by Rust. Prepared from the validated Rust recording, with raw inputs durably referenced. Robot properties and command bounds unchanged. Requested outputs and saturation counts retained in replayable policy state.'});
