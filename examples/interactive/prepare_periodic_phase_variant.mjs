// Robot-independent artifact preparation; shared Rust performs curve shifts.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {isDeepStrictEqual} from 'node:util';
const read=p=>JSON.parse(fs.readFileSync(p));
const write=(p,x)=>fs.writeFileSync(p,JSON.stringify(x)+'\n',{flag:'wx'});
const pin=path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});

export function preparePhaseVariant(spec,requestedCycles,root) {
  assert.equal(spec.version,1);assert.equal(requestedCycles.length,spec.groups.length);
  assert(requestedCycles.every(x=>Number.isFinite(x)&&x>=-0.5&&x<=0.5));
  const source=read(spec.scene),scene=structuredClone(source),parameters=scene.controller.parameters;
  assert.equal(source.robot.source.cad_sha256,spec.cad_sha256);
  const names=Object.entries(parameters.motor_indices).sort((a,b)=>a[1]-b[1]).map(([name])=>name);
  const all=spec.groups.flatMap(g=>g.targets);assert.equal(new Set(all).size,names.length);
  assert.deepEqual([...all].sort(),[...names].sort());
  assert.equal(new Set(spec.groups.map(g=>g.name)).size,spec.groups.length);
  assert(spec.trajectory_parameters.length>0);assert.equal(new Set(spec.trajectory_parameters).size,spec.trajectory_parameters.length);
  const reference=parameters[spec.trajectory_parameters[0]],count=reference.keyframes.length-1,period=reference.keyframes.at(-1).time_s;
  assert(count>=4&&period>0);assert.equal(reference.interpolation,'periodic_cubic_b_spline');
  const offsets=requestedCycles.map(x=>Math.round(x*count));
  const integers=names.map(name=>offsets[spec.groups.findIndex(g=>g.targets.includes(name))]);
  fs.mkdirSync(root);write(root+'/integer-shifts.json',integers);
  const executions=[];
  for(const name of spec.trajectory_parameters){
    assert(/^[a-z_]+$/.test(name),'plain parameter names required for artifact paths');
    const curve=parameters[name];assert.equal(curve.interpolation,'periodic_cubic_b_spline');
    assert.equal(curve.keyframes.length,count+1);assert.equal(curve.keyframes[0].values.length,names.length);
    assert(curve.keyframes.every((k,i)=>k.time_s===reference.keyframes[i].time_s),'shifted signal grids must align exactly');
    const input=root+'/'+name+'.original.json',output=root+'/'+name+'.shifted.json';write(input,curve);
    const args=[input,root+'/integer-shifts.json',output],r=spawnSync(spec.binary,args,{encoding:'utf8'});
    executions.push({parameter:name,binary:spec.binary,args,exit_code:r.status,signal:r.signal,error:r.error?.message??null,stderr:r.stderr});
    write(root+'/'+name+'.execution.json',executions.at(-1));assert.equal(r.status,0,r.stderr);
    parameters[name]=read(output);
  }
  const restored=structuredClone(scene);for(const name of spec.trajectory_parameters)restored.controller.parameters[name]=source.controller.parameters[name];
  assert(isDeepStrictEqual(restored,source),'unexpected robot/world/policy mutation');
  write(root+'/scene.json',scene);
  const report={version:1,requested_cycles:requestedCycles,integer_group_shifts:offsets,executed_cycles:offsets.map(x=>x/count),
    controls:count,reference_period_s:period,reference_shift_resolution_s:period/count,
    maximum_reference_time_rounding_error_s:Math.max(...requestedCycles.map((x,i)=>Math.abs(x-offsets[i]/count)*period)),
    groups:spec.groups,executions,
    scope:'Per-group phases are rounded to the nearest existing control interval (ties toward positive infinity). Rust then exactly permutes periodic controls, preserving each signal shape and derivative curve. Reference and feedforward signals share the shift. Offsets refer to the reference clock, not elapsed simulation time. Actual contact remains determined by unchanged runtime physics.'};
  write(root+'/report.json',report);
  write(root+'/inputs.json',{version:1,files:[spec.scene,spec.binary,import.meta.filename,root+'/scene.json',root+'/integer-shifts.json',root+'/report.json'].map(pin)});
  return report;
}
