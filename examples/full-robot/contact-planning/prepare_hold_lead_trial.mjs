// Experiment assembly; unchanged Rhai/Rust controller and dynamics execute it.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const [source,name]=process.argv.slice(2);
assert(source&&/^[a-z0-9-]+$/.test(name),'source prefix and fresh name required');
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const scene=read(source+'.scene.json'),config=read(source+'.config.json'),actions=read(source+'.actions.json');
const original=scene.controller.parameters.velocity_lead_s.slice();
assert(scene.period_s===.02&&original.length===12&&original.every(v=>Math.abs(v-.02)<1e-15),
 'this matched experiment requires the original D/K=.020 s lead and .020 s holds');
scene.controller.parameters.velocity_lead_s=original.map(v=>v+scene.period_s/2);
const prefix='runs/contact-planning/'+name,dir='examples/full-robot/contact-planning/';
const speedIndex=scene.controller.inputs.findIndex(i=>i.name==='command.forward_speed');
const speed=Math.max(...actions.map(a=>Math.abs(a[speedIndex])));
const files=['scene','config','actions'].map(kind=>prefix+'.'+kind+'.json');
assert([...files,dir+name+'-trial.json'].every(p=>!fs.existsSync(p)),'fresh outputs required');
for(const [i,value] of [scene,config,actions].entries())fs.writeFileSync(files[i],JSON.stringify(value)+'\n',{flag:'wx'});
fs.writeFileSync(dir+name+'-trial.json',JSON.stringify({prefix,source,command_speed_m_s:speed,
 windows_s:[[.8,3],[4.2,6.4]],task:'examples/full-robot/fast-wasd/task.json',
 original_velocity_lead_s:original,new_velocity_lead_s:scene.controller.parameters.velocity_lead_s,
 sources:['scene','config','actions'].map(kind=>({path:source+'.'+kind+'.json',sha256:hash(source+'.'+kind+'.json')})),
 files:files.map(path=>({path,sha256:hash(path)})),
 scope:'Matched runtime test of mean linear-reference advance over one zero-order hold: added lead T/2=.010 s, giving D/K+T/2=.030 s. This is a first-order hold correction, not exact compensation for curved paths, saturation, impacts or clock acceleration. All other scene/config/actions and all physical gates remain unchanged.'
},null,2)+'\n',{flag:'wx'});
