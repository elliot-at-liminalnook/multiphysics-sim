// Test the hold-compensated controller toward and beyond the current reference's
// 0.11077 m/s no-load shaft-rate screen. The screen is not a hard speed limit.
import fs from 'node:fs';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',source='runs/speed-ceiling/validation/sampled-gain0.5-lookahead10ms';
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const catalog=read(`${d}/validation-cases.json`);
for(const speed of process.argv.slice(2).map(Number)){
 if(!Number.isFinite(speed)||speed<=0)throw Error('positive speed required');
 const name=`synchronized-cadence-${speed*1000}-human-fine`,prefix=`runs/speed-ceiling/validation/${name}`;
 if(fs.existsSync(`${prefix}.scene.json`))throw Error(`refusing overwrite ${name}`);
 const scene=read(`${source}.scene.json`),config=read(`${source}.config.json`);
 const index=scene.controller.inputs.findIndex(ch=>ch.name==='command.forward_speed');
 scene.controller.inputs[index].lower=-speed;scene.controller.inputs[index].upper=speed;
 const actions=read(`${source}.actions.json`).map(row=>{row[index]*=speed/.1;return row;});
 const entry={name,kind:'human',prefix,duration_s:20,step_s:config.step_s,command_speed_m_s:speed,source,
  scope:'Unchanged 52 mm reference, gain 0.5, damping plus midpoint hold lead. Only requested cadence changes; actuator physics stays fixed.'};
 for(const [suffix,value] of [['scene',scene],['config',config],['actions',actions]]){
  fs.writeFileSync(`${prefix}.${suffix}.json`,JSON.stringify(value)+'\n');entry[`${suffix}_sha256`]=sha(`${prefix}.${suffix}.json`);
 }
 catalog.rows.push(entry);
}
fs.writeFileSync(`${d}/validation-cases.json`,JSON.stringify(catalog,null,2)+'\n');
