// Constant-velocity hold analysis: mean q over a held interval advances by
// v*dt/2. Target lead D/K + dt/2 cancels nominal damping and that hold lag.
// This changes controller commands only; actuator K/D/torque limits stay fixed.
import fs from 'node:fs';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const catalog=read(`${d}/validation-cases.json`),source='runs/speed-ceiling/validation/human-0p625ms';
for(const [gain,lookahead] of [[0,0],[.5,.01],[0,.01]]){
 const name=`sampled-gain${gain}-lookahead${lookahead*1000}ms`,prefix=`runs/speed-ceiling/validation/${name}`;
 if(fs.existsSync(`${prefix}.scene.json`))throw Error(`refusing overwrite ${name}`);
 const scene=read(`${source}.scene.json`),c=read(`${source}.config.json`);
 scene.controller.inputs.forEach(ch=>{if(ch.name==='command.tracking_gain')ch.lower=ch.upper=ch.initial=gain;});
 scene.controller.parameters.velocity_lead_s=c.motors.effective.components.map(m=>m.parameters.damping/m.parameters.stiffness+lookahead);
 const gainIndex=scene.controller.inputs.findIndex(ch=>ch.name==='command.tracking_gain');
 const actions=read(`${source}.actions.json`).map(row=>{row[gainIndex]=gain;return row;});
 const row={name,kind:'human',prefix,duration_s:20,step_s:.000625,command_speed_m_s:.1,source,
  controller_tracking_gain:gain,hold_lookahead_s:lookahead,scope:'Outer sampled proportional correction and hold-midpoint target lead. No extra force, actuator authority or state oracle.'};
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',actions]]){fs.writeFileSync(`${prefix}.${suffix}.json`,JSON.stringify(value)+'\n');row[`${suffix}_sha256`]=sha(`${prefix}.${suffix}.json`);}
 catalog.rows.push(row);
}
fs.writeFileSync(`${d}/validation-cases.json`,JSON.stringify(catalog,null,2)+'\n');
