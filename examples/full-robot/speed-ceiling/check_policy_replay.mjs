import fs from 'node:fs';import {spawnSync} from 'node:child_process';import os from 'node:os';import path from 'node:path';
const input='runs/speed-ceiling/validation/synchronized-human-0p3125ms.native.json',c=JSON.parse(fs.readFileSync(input));
const thin={completed:c.completed,error:c.error,recording:{scene:{controller:c.recording.scene.controller},config:{policy:{}},seed:c.recording.seed},
 metadata:{policy_contract:c.metadata.policy_contract},frames:c.frames.map(f=>({time_s:f.time_s,policy:f.policy,servo_targets_rad:f.servo_targets_rad}))};
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'policy-replay-'));
const bin=(process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples')+'/replay_policy_state',checks=[];
try {
 for(const [name,edit,accept] of [
  ['valid',x=>{},true],['missing-middle',x=>x.frames.splice(500,1),false],
  ['command-mismatch',x=>{const key=Object.keys(x.frames[200].policy.targets)[0];x.frames[200].policy.targets[key]+=.0001;},false],
  ['wrong-unit',x=>{x.metadata.policy_contract.observations[0].unit='m';},false],
  ['missing-tail-policy',x=>{x.frames.at(-1).policy=null;},false],
  ['conflicting-duplicate',x=>{const extra=structuredClone(x.frames[200]);extra.policy.observations['command.forward_speed']+=.01;x.frames.splice(201,0,extra);},false]
 ]){
  const value=structuredClone(thin);edit(value);const file=path.join(dir,name+'.json');fs.writeFileSync(file,JSON.stringify(value));
  const r=spawnSync(bin,[file],{encoding:'utf8',maxBuffer:16*1024*1024});
  if((r.status===0)!==accept)throw Error(name+': '+r.stderr);
  checks.push({name,expected_acceptance:accept,exit:r.status,error:r.stderr.trim(),maximum_command_error_rad:accept?JSON.parse(r.stdout).maximum_command_error_rad:null});
 }
 fs.writeFileSync('examples/full-robot/speed-ceiling/policy-replay-checks.json',JSON.stringify({source:input,checks},null,2)+'\n');
 console.log({checks:checks.length,passed:true});
}finally{fs.rmSync(dir,{recursive:true});}
