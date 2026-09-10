// Time-scale the accepted short-stroke path instead of increasing its reach.
import fs from 'node:fs';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',base='examples/full-robot/fast-wasd';
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const catalog=read(`${d}/clearance-trials.json`);
for(const speed of process.argv.length>2?process.argv.slice(2).map(Number):[.08,.09,.1,.11,.125,.15]){
 if(!Number.isFinite(speed)||speed<=0)throw Error('positive command speed required');
 const name=`short-stroke-cadence-${speed*1000}`,prefix=`runs/speed-ceiling/clearance/${name}`;
 if(fs.existsSync(`${prefix}.scene.json`))throw Error(`refusing overwrite ${name}`);
 const scene=read(`${base}/braked-5ms.scene.json`),c=read(`${base}/braked-5ms.config.json`);
 scene.duration_s=6;scene.controller.inputs.forEach(ch=>{if(ch.name==='command.forward_speed'){ch.lower=-speed;ch.upper=speed;}});
 c.steps=1200;c.report_every=4;
 const actions=Array.from({length:300},(_,i)=>scene.controller.inputs.map(ch=>ch.name==='command.forward_speed'?(i>=20&&i<260?speed:0):ch.name==='command.packet_sequence'?i+1:ch.initial));
 const row={name,prefix,hip:0,speed,lift:8,gain:.5,planning_exit:0,source_plan:`${base}/hip0-65.plan.json`,
  source_plan_sha256:sha(`${base}/hip0-65.plan.json`),period_s:.8*.065/speed,method:'Time scaling of accepted 52 mm stride with unchanged controller damping feedforward and braking'};
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',actions]]){
  fs.writeFileSync(`${prefix}.${suffix}.json`,JSON.stringify(value)+'\n');row[`${suffix}_sha256`]=sha(`${prefix}.${suffix}.json`);
 }
 catalog.rows.push(row);
}
fs.writeFileSync(`${d}/clearance-trials.json`,JSON.stringify(catalog,null,2)+'\n');
