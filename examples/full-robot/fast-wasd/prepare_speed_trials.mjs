import fs from 'node:fs';
const d='examples/full-robot/fast-wasd',old='examples/full-robot/gait-exploration';
const read=(dir,name)=>JSON.parse(fs.readFileSync(`${dir}/${name}.json`));
const refined=read(d,'refined.scene'),base=read(old,'live-pair.scene'),config=read(old,'live-pair.config');
const definitions=[];
for(const method of ['stride','cadence']) for(const speed of [.05,.1]) {
 const name=`${method}-${speed*1000}`,scene=structuredClone(base),c=structuredClone(config);
 const plan=read(method==='stride'?d:old,method==='stride'?`opposite-pair-${speed*1000}mm.plan`:'opposite-pair-clear-flight.plan');
 scene.robot=structuredClone(refined.robot);
 scene.controller.parameters.samples=plan.trajectory.keyframes.filter(k=>k.time_s>=.4-1e-9&&k.time_s<=1.2+1e-9).map(k=>k.values);
 const samples=scene.controller.parameters.samples;
 if(samples.length!==41||samples[0].some((x,i)=>Math.abs(x-samples.at(-1)[i])>1e-7))throw Error('nonperiodic cycle');
 scene.controller.parameters.nominal_speed_m_s=method==='stride'?speed:.025;
 scene.controller.inputs.forEach(ch=>{if(ch.name==='command.forward_speed'){ch.lower=-speed;ch.upper=speed;ch.initial=0}});
 scene.duration_s=6;
 c.initial_coordinates=samples[0];c.motors.servos.forEach((s,i)=>s.target_rad=samples[0][i]);
 c.step_s=.00125;c.steps=4800;c.report_every=16;
 const actions=Array.from({length:300},(_,i)=>scene.controller.inputs.map(ch=>ch.name==='command.forward_speed'?(i>=20&&i<260?speed:0):ch.initial));
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',actions]])fs.writeFileSync(`${d}/${name}.${suffix}.json`,JSON.stringify(value)+'\n');
 definitions.push({name,method,command_speed_m_s:speed,period_s:.8*scene.controller.parameters.nominal_speed_m_s/speed,source_plan:method==='stride'?`${d}/opposite-pair-${speed*1000}mm.plan.json`:`${old}/opposite-pair-clear-flight.plan.json`});
}
fs.copyFileSync(`${old}/live-pair.task.json`,`${d}/task.json`);
fs.writeFileSync(`${d}/speed-trials.json`,JSON.stringify({definitions,scope:'Live command-driven Rhai controller; no reference trajectory or privileged feedback consumed. 0.4 s hold, 4.8 s forward, 0.8 s stop. Development screening only.',gates:{completed:true,maximum_tilt_rad:.1,maximum_stop_drift_m:.003,maximum_loaded_material_motion_ratio:.05,independent_collision_audit:true}},null,2)+'\n');
