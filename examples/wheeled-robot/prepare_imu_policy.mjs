// Package the original CAD model and versioned Rust-runtime recipe unchanged.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const [directory,bundle,profile='imu']=process.argv.slice(2);
assert(directory&&['imu','velocity'].includes(profile),'usage: prepare_imu_policy.mjs output-directory [isolated-viewer-bundle] [imu|velocity]');
const root=path.dirname(import.meta.filename);
const read=name=>JSON.parse(fs.readFileSync(path.join(root,name)));
const scene={version:1,robot:read('baseline/robot.simrobot.json'),
  options:{contact:false,flex:false},period_s:0.003,duration_s:0.03,
  controller:{sources:{entry:'imu-controller.rhai',files:{'imu-controller.rhai':fs.readFileSync(path.join(root,'imu-controller.rhai'),'utf8')}},
    parameters:{},inputs:['left','right'].map(name=>({name:'command.'+name,kind:'Angle',lower:-0.1,upper:0.1,initial:0}))}};
const config=read('imu-policy.config.json'),task=read('imu-policy.task.json');
if(profile==='velocity') {
  scene.controller={sources:{entry:'velocity-controller.rhai',files:{'velocity-controller.rhai':fs.readFileSync(path.join(root,'velocity-controller.rhai'),'utf8')}},
    parameters:{period_s:scene.period_s,initial_left:config.motors.servos[0].target_rad,initial_right:config.motors.servos[1].target_rad},
    inputs:read('velocity-controller.inputs.json')};
}
const data={scene,config,task};
fs.mkdirSync(directory,{recursive:true});
for(const [name,value] of Object.entries({scene,config,task,data,scope:{version:1,
  profile,scope:'30 ms sensor/controller/environment API acceptance without contact. Original CAD physics and IMU retained. No locomotion, speed, hardware or realtime qualification.'}})) {
  fs.writeFileSync(path.join(directory,name+'.json'),JSON.stringify(value)+'\n',{flag:'wx'});
}
if(bundle){
  const catalogPath=path.join(bundle,'catalog.json'),catalog=JSON.parse(fs.readFileSync(catalogPath));
  const id='wheeled-'+profile+'-policy'; assert(!catalog.presets.some(p=>p.id===id));
  fs.mkdirSync(path.join(bundle,'data'),{recursive:true});
  fs.copyFileSync(path.join(directory,'data.json'),path.join(bundle,'data',id+'.json'),fs.constants.COPYFILE_EXCL);
  catalog.presets.push({id,label:'Wheeled authored IMU policy',mode:'embedded',task:true,path:'data/'+id+'.json'});
  fs.writeFileSync(catalogPath,JSON.stringify(catalog)+'\n');
}
