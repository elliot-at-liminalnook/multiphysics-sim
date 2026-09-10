import fs from 'node:fs';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/fast-wasd',old='examples/full-robot/gait-exploration',read=(p)=>JSON.parse(fs.readFileSync(p));
const catalog=read(`${d}/speed-trials.json`),status=fs.existsSync(`${d}/landing-planning-status.json`)?read(`${d}/landing-planning-status.json`):[];
const smooth=u=>{u=Math.max(0,Math.min(1,u));return u*u*u*(10+u*(-15+6*u))};
for(const centered of [false,true])for(const speed of [.05,.075,.1]){
 const name=`landing-${speed*1000}${centered?'-centered-transition':''}`,p=read(`${old}/opposite-pair.planning.json`);
 if(fs.existsSync(`${d}/${name}.planning.json`))continue;
 const stride=speed*.8,lift=.008,swing=.32;
 for(const k of p.base_displacements_world_m.keyframes)k.values[0]*=stride/.02;
 for(const k of p.displacements_world_m.keyframes){
  const elapsed=Math.max(0,Math.min(4.8,k.time_s-.4)),transfer=Math.min(11,Math.floor((elapsed+1e-9)/.4));
  const u=(elapsed-transfer*.4-.04)/swing,active=transfer%2===0?[0,2]:[1,3];
  for(let leg=0;leg<4;leg++){
   const group=leg===0||leg===2?0:1,completed=Math.floor(transfer/2)+(group<transfer%2?1:0);
   k.values[3*leg]=stride*(completed+(active.includes(leg)?smooth(u):0))+(centered?(group===0?-1:1)*stride/4*smooth(k.time_s/.4):0);
   k.values[3*leg+2]=active.includes(leg)&&u>0&&u<1?lift*(u<.25?smooth(u/.25):u>.75?smooth((1-u)/.25):1):0;
  }
 }
 fs.writeFileSync(`${d}/${name}.planning.json`,JSON.stringify(p)+'\n');
 const o=fs.openSync(`${d}/${name}.plan.json`,'wx'),e=fs.openSync(`${d}/${name}.plan-error.txt`,'wx');
 const r=spawnSync('/Users/elliot/physics-simulator/target/gait-exploration/release/examples/plan_marker_motion',[`${d}/refined.scene.json`,`${old}/workspace-markers.json`,`${d}/${name}.planning.json`],{stdio:['ignore',o,e],timeout:60000});fs.closeSync(o);fs.closeSync(e);
 status.push({name,exit:r.status,error:r.error?.message});console.log(status.at(-1));
 if(r.status!==0)continue;
 const plan=read(`${d}/${name}.plan.json`),scene=read(`${d}/stride-50.scene.json`),c=read(`${d}/stride-50.config.json`);
 const samples=plan.trajectory.keyframes.filter(k=>k.time_s>=.4-1e-9&&k.time_s<=1.2+1e-9).map(k=>k.values);
 if(samples.length!==41||samples[0].some((x,i)=>Math.abs(x-samples.at(-1)[i])>1e-7))throw Error('nonperiodic cycle');
 Object.assign(scene.controller.parameters,{samples,nominal_speed_m_s:speed,swing_start_s:.04,swing_end_s:.36});
 const files=scene.controller.sources.files;for(const key of Object.keys(files))files[key]=files[key].replaceAll('0.08','p.swing_start_s').replaceAll('0.32','p.swing_end_s');
 scene.controller.inputs.forEach(ch=>{if(ch.name==='command.forward_speed'){ch.lower=-speed;ch.upper=speed;ch.initial=0}});
 c.initial_coordinates=samples[0];c.motors.servos.forEach((s,i)=>s.target_rad=samples[0][i]);
 const actions=Array.from({length:300},(_,i)=>scene.controller.inputs.map(ch=>ch.name==='command.forward_speed'?(i>=20&&i<260?speed:0):ch.initial));
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',actions]])fs.writeFileSync(`${d}/${name}.${suffix}.json`,JSON.stringify(value)+'\n');
 catalog.definitions.push({name,method:'longer swing with earlier lift and later landing',command_speed_m_s:speed,period_s:.8,stride_m:stride,lift_m:lift,swing_s:swing,centered_stroke:centered,peak_world_swing_speed_m_s:1.875*stride/swing,source_plan:`${d}/${name}.plan.json`});
}
fs.writeFileSync(`${d}/landing-planning-status.json`,JSON.stringify(status,null,2)+'\n');
fs.writeFileSync(`${d}/speed-trials.json`,JSON.stringify(catalog,null,2)+'\n');
