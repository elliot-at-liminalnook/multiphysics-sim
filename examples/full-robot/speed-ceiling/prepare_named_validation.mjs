import fs from 'node:fs';import crypto from 'node:crypto';
import {cadYawRatios,cadYawCoordinates} from './cad_yaw_ratios.mjs';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const catalog=read(`${d}/validation-cases.json`),trials=read(`${d}/clearance-trials.json`);
const deriveYaw=process.argv.includes('--cad-yaw');
const fullYaw=process.argv.includes('--full-cad-yaw');
if(deriveYaw&&fullYaw)throw Error('choose one steering derivation');
for(const sourceName of process.argv.slice(2).filter(s=>!['--cad-yaw','--full-cad-yaw'].includes(s))){
 const source=trials.rows.find(r=>r.name===sourceName);if(!source||source.planning_exit!==0)throw Error('compiled source required');
 const name=`${sourceName}${fullYaw?'-full-cad-yaw':deriveYaw?'-cad-yaw':''}-human-fine`,prefix=`runs/speed-ceiling/validation/${name}`;
 if(fs.existsSync(`${prefix}.scene.json`))throw Error(`refusing overwrite ${name}`);
 const scene=read(`${source.prefix}.scene.json`),c=read(`${source.prefix}.config.json`);
 scene.duration_s=20;c.step_s=.000625;c.steps=32000;c.report_every=32;
 if(deriveYaw)scene.controller.parameters.yaw_jacobian_ratios=cadYawRatios(`${source.prefix}.scene.json`,c,prefix);
 if(fullYaw){
  scene.controller.parameters.yaw_coordinates=cadYawCoordinates(`${source.prefix}.scene.json`,c,prefix);
  const key=scene.controller.sources.entry;let code=scene.controller.sources.files[key];
  const old='if active_pair&&u>p.swing_start_s&&u<p.swing_end_s {state.yaw_offsets[leg]*=0.65;}\n  else {state.yaw_offsets[leg]=(state.yaw_offsets[leg]-p.yaw_jacobian_ratios[leg]*yaw*dt).max(-0.05).min(0.05);}';
  if(code.split(old).length!==2)throw Error('expected baseline hip steering law');
  code=code.replace(old,'for joint in leg*3..leg*3+3 {\n   if active_pair&&u>p.swing_start_s&&u<p.swing_end_s {state.yaw_offsets[joint]*=0.65;}\n   else {state.yaw_offsets[joint]=(state.yaw_offsets[joint]-p.yaw_coordinates[joint]*yaw*dt).max(-0.05).min(0.05);}\n  }');
  code=code.replace('state.yaw_offsets=[0.0,0.0,0.0,0.0];',`state.yaw_offsets=[${Array(12).fill('0.0').join(',')}];`);
  code=code.replace('if j%3==0 {target+=state.yaw_offsets[j/3];}','target+=state.yaw_offsets[j];');
  scene.controller.sources.files[key]=code;
 }
 const index=scene.controller.inputs.findIndex(ch=>ch.name==='command.forward_speed');
 const actions=read('examples/full-robot/fast-wasd/braked-5ms.actions.json').map(row=>{row[index]*=source.speed/.065;return row;});
 const row={name,kind:'human',prefix,duration_s:20,step_s:.000625,command_speed_m_s:source.speed,source:source.prefix,cad_yaw_derived:deriveYaw||fullYaw,full_joint_yaw:fullYaw};
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',actions]]){fs.writeFileSync(`${prefix}.${suffix}.json`,JSON.stringify(value)+'\n');row[`${suffix}_sha256`]=sha(`${prefix}.${suffix}.json`);}
 catalog.rows.push(row);
}
fs.writeFileSync(`${d}/validation-cases.json`,JSON.stringify(catalog,null,2)+'\n');
