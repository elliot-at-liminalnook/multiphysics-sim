import fs from 'node:fs';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/fast-wasd',old='examples/full-robot/gait-exploration',read=p=>JSON.parse(fs.readFileSync(p));
const catalog=read(`${d}/speed-trials.json`),status=[];
for(const [hip,speed] of [[0,.06],[0,.065],[30,.05],[30,.075],[30,.1],[45,.05],[45,.075],[45,.1]]) {
 const name=`hip${hip}-${speed*1000}`,p=read(`${d}/landing-50.planning.json`);
 for(let i=0;i<4;i++)p.initial_coordinates[3*i]=hip*Math.PI/180;
 for(const trajectory of [p.displacements_world_m,p.base_displacements_world_m])for(const k of trajectory.keyframes)for(let i=0;i<k.values.length;i+=3)k.values[i]*=speed/.05;
 fs.writeFileSync(`${d}/${name}.planning.json`,JSON.stringify(p)+'\n');
 const o=fs.openSync(`${d}/${name}.plan.json`,'wx'),e=fs.openSync(`${d}/${name}.plan-error.txt`,'wx');
 const r=spawnSync('/Users/elliot/physics-simulator/target/gait-exploration/release/examples/plan_marker_motion',[`${d}/refined.scene.json`,`${old}/workspace-markers.json`,`${d}/${name}.planning.json`],{stdio:['ignore',o,e],timeout:60000});fs.closeSync(o);fs.closeSync(e);
 status.push({name,exit:r.status,error:r.error?.message});console.log(status.at(-1));if(r.status!==0)continue;
 const plan=read(`${d}/${name}.plan.json`),scene=read(`${d}/landing-50.scene.json`),c=read(`${d}/landing-50.config.json`);
 const samples=plan.trajectory.keyframes.filter(k=>k.time_s>=.4-1e-9&&k.time_s<=1.2+1e-9).map(k=>k.values);
 if(samples.length!==41||samples[0].some((x,i)=>Math.abs(x-samples.at(-1)[i])>1e-7))throw Error('nonperiodic cycle');
 Object.assign(scene.controller.parameters,{samples,nominal_speed_m_s:speed});
 scene.controller.inputs.forEach(ch=>{if(ch.name==='command.forward_speed'){ch.lower=-speed;ch.upper=speed;ch.initial=0}});
 c.initial_coordinates=samples[0];c.motors.servos.forEach((s,i)=>s.target_rad=samples[0][i]);
 const actions=Array.from({length:300},(_,i)=>scene.controller.inputs.map(ch=>ch.name==='command.forward_speed'?(i>=20&&i<260?speed:0):ch.initial));
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',actions]])fs.writeFileSync(`${d}/${name}.${suffix}.json`,JSON.stringify(value)+'\n');
 catalog.definitions.push({name,method:'CAD hip rotation shares forward stroke between belt and worm',hip_initial_deg:hip,command_speed_m_s:speed,period_s:.8,stride_m:speed*.8,lift_m:.008,swing_s:.32,source_plan:`${d}/${name}.plan.json`});
}
fs.writeFileSync(`${d}/hip-planning-status.json`,JSON.stringify(status,null,2)+'\n');fs.writeFileSync(`${d}/speed-trials.json`,JSON.stringify(catalog,null,2)+'\n');
