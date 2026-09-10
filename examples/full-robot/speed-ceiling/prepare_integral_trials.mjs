// A bounded integral estimates the steady target offset needed under load.
// Uses the existing shared Rust control.angle_integral kernel through Rhai.
import fs from 'node:fs';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const catalog=read(`${d}/clearance-trials.json`);
for(const [sourceName,speed,gain] of [
 ['belt-short-hip0-v100',.11,.5],['belt-short-hip0-v100',.11,2],['belt-short-hip0-v100',.11,5],
 ['belt-short-hip30-v125',.125,.5],['belt-short-hip30-v125',.125,2]
]){
 const source=catalog.rows.find(r=>r.name===sourceName);if(!source||source.planning_exit!==0)throw Error('compiled source required');
 const name=`integral-hip${source.hip}-v${speed*1000}-gain${gain}`,prefix=`runs/speed-ceiling/clearance/${name}`;
 if(fs.existsSync(`${prefix}.scene.json`))throw Error(`refusing overwrite ${name}`);
 const scene=read(`${source.prefix}.scene.json`),config=read(`${source.prefix}.config.json`),entry=scene.controller.sources.entry;
 let code=scene.controller.sources.files[entry];
 const before='commands[name]=target+sensors["command.tracking_gain"]*(target-sensors[joint+".angle"])+p.velocity_lead_s[j]*velocity;';
 if(code.split(before).length!==2)throw Error('expected unique baseline command law');
 code=code.replace('let p=parameters();','let p=parameters();\n if !state.contains("bias") { state.bias=[0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0]; }');
 code=code.replace(before,'state.bias[j]=angle_integral_update(state.bias[j],target-sensors[joint+".angle"],true,p.angle_integral);\n  '+before.replace(';','+state.bias[j];'));
 scene.controller.sources.files[entry]=code;
 scene.controller.parameters.angle_integral={period_s:.02,integral_gain_per_s:gain,leak_rate_per_s:0,maximum_bias_rad:.06,maximum_rate_rad_s:.1};
 const index=scene.controller.inputs.findIndex(ch=>ch.name==='command.forward_speed');scene.controller.inputs[index].lower=-speed;scene.controller.inputs[index].upper=speed;
 const actions=read(`${source.prefix}.actions.json`).map(row=>{row[index]*=speed/source.speed;return row;});
 const row={name,prefix,hip:source.hip,speed,lift:source.lift,gain:source.gain,planning_exit:0,source_plan:`${source.prefix}.plan.json`,source_plan_sha256:sha(`${source.prefix}.plan.json`),
  source_scene:source.source_scene,integral:scene.controller.parameters.angle_integral,method:'Existing bounded Rust angular integral adds load bias to the hold-compensated sampled controller; original phase/braking/lease and actuator limits retained.'};
 for(const [suffix,value] of [['scene',scene],['config',config],['actions',actions]]){fs.writeFileSync(`${prefix}.${suffix}.json`,JSON.stringify(value)+'\n');row[`${suffix}_sha256`]=sha(`${prefix}.${suffix}.json`);}
 catalog.rows.push(row);
}
fs.writeFileSync(`${d}/clearance-trials.json`,JSON.stringify(catalog,null,2)+'\n');
