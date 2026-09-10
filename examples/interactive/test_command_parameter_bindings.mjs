import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {loadExperiment} from './run_affine_speed_search.mjs';

function fixture(t) {
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'command-binding-'));
  t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  const write=(name,value)=>{const p=path.join(root,name+'.json');fs.writeFileSync(p,JSON.stringify(value));return p;};
  const reference={keyframes:[{time_s:0,values:[0]},{time_s:1,values:[0]}]};
  const inputs=[['command.forward_speed','LinearVelocity'],['command.tracking_gain','Dimensionless'],['command.packet_sequence','Dimensionless'],['command.yaw_rate','AngularVelocity']].map(([name,kind])=>({name,kind,initial:0,lower:-1,upper:1}));
  const schema=inputs.map((c,i)=>({name:c.name,kind:c.kind,unit:['m/s','1','1','rad/s'][i]}));
  const spec={version:1,cad_sha256:'synthetic-binding-only',
    scene:write('scene',{duration_s:1,robot:{source:{cad_sha256:'synthetic-binding-only'}},controller:{inputs,parameters:{motor_indices:{'joint.target':0},trajectory:reference}}}),
    config:write('config',{step_s:.001,steps:1000,policy:{}}),task:write('task',{period_s:.02}),
    actions:write('actions',Array.from({length:50},(_,i)=>[.2,-.4,i+1,.09])),reference:write('reference',reference),
    command_schema:write('schema',schema),extra_command_parameters:[{name:'yaw',input:'command.yaw_rate'}],
    groups:[{name:'group',targets:['joint.target']}],centers_rad:[0],baseline_values:[.2,-.4,1,1,.09],
    problem:{parameters:[['command_speed','m/s'],['tracking_gain','1'],['group_amplitude','1'],['group_lead','1'],['yaw','rad/s']].map(([name,unit])=>({name,unit,bounds:[-1,1]})),constraints:[{name:'fall'}]}};
  return {spec,schema,write};
}

test('binds an extra command by exported name and units while preserving the old path',t=>{
  const {spec}=fixture(t),source=loadExperiment(spec);
  assert.deepEqual(source.extraCommands,[{inputIndex:3,parameterIndex:4}]);
  const legacy=structuredClone(spec);delete legacy.extra_command_parameters;delete legacy.command_schema;
  legacy.problem.parameters.pop();legacy.baseline_values.pop();
  assert.deepEqual(loadExperiment(legacy).extraCommands,[]);
});

test('rejects wrong units, quantity kinds, duplicate inputs and heartbeat control',t=>{
  const {spec,schema,write}=fixture(t);
  const badUnit=structuredClone(spec);badUnit.problem.parameters[4].unit='m/s';
  assert.throws(()=>loadExperiment(badUnit),/unit mismatch/);
  const badKind=structuredClone(spec);const changed=structuredClone(schema);changed[3].kind='LinearVelocity';badKind.command_schema=write('bad-schema',changed);
  assert.throws(()=>loadExperiment(badKind),/quantity kind mismatch/);
  const duplicate=structuredClone(spec);duplicate.extra_command_parameters.push({name:'second_yaw',input:'command.yaw_rate'});duplicate.problem.parameters.push({name:'second_yaw',unit:'rad/s',bounds:[-1,1]});duplicate.baseline_values.push(.09);
  assert.throws(()=>loadExperiment(duplicate),/duplicate extra command binding/);
  const heartbeat=structuredClone(spec);heartbeat.extra_command_parameters[0].input='command.packet_sequence';
  assert.throws(()=>loadExperiment(heartbeat),/non-heartbeat/);
  const missing=structuredClone(spec);delete missing.command_schema;
  assert.throws(()=>loadExperiment(missing),/exported channel schema/);
});
