// Transport the durable CAD export unchanged into a shared runtime scene.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const [directory, bundle, profile='sensors'] = process.argv.slice(2);
assert(directory && ['sensors','contact'].includes(profile), 'usage: prepare_sensor_smoke.mjs output-directory [viewer-bundle] [sensors|contact]');
fs.mkdirSync(directory, {recursive:true});
const root = path.dirname(import.meta.filename);
const robot = JSON.parse(fs.readFileSync(root+'/baseline/robot.simrobot.json'));
const config = JSON.parse(fs.readFileSync(root+(profile==='contact'?'/contact-smoke.config.json':'/sensor-smoke.config.json')));
const scene = {version:1, robot, options:{contact:profile==='contact', flex:false}, period_s:0.02, duration_s:0.02};
for (const [name,value] of [['scene.json',scene],['config.json',config],['data.json',{scene,config}],
  ['scope.json',{version:1,profile,simulated_s:config.steps*config.step_s,scope:'Short sensor/coordinate portability case. Original CAD properties, passive axle and IMU retained. Contact mode and zero winding-voltage boundaries explicitly selected. No locomotion, calibration or realtime qualification.'}]]) {
  fs.writeFileSync(path.join(directory,name),JSON.stringify(value)+'\n',{flag:'wx'});
}
if (bundle) {
  const catalogPath=path.join(bundle,'catalog.json');
  const catalog=JSON.parse(fs.readFileSync(catalogPath));
  const id='wheeled-'+profile;
  assert(!catalog.presets.some(p=>p.id===id), 'preset already installed');
  fs.mkdirSync(path.join(bundle,'data'),{recursive:true});
  fs.copyFileSync(path.join(directory,'data.json'),path.join(bundle,'data',id+'.json'),fs.constants.COPYFILE_EXCL);
  catalog.presets.push({id,label:'Wheeled '+profile+' smoke test',mode:'embedded',path:'data/'+id+'.json'});
  fs.writeFileSync(catalogPath,JSON.stringify(catalog)+'\n');
}
