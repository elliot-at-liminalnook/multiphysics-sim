// Preserve the shared historical human/command-loss schedules and physical model.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const d='examples/full-robot/contact-planning',source=process.argv[2]??'runs/contact-planning/compiled184-screen',id=process.argv[3]??'compiled184';
if(!/^[a-z0-9-]+$/.test(id))throw Error('named validation family required');
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const settings=process.argv[4]?read(process.argv[4]):{};
assert(Object.keys(settings).every(k=>['command_speed_m_s','constant_inputs','include_finer_case'].includes(k)));
const rows=[];
for(const [suffix,kind,duration,step,actionsName] of [
 ['human-1p25ms','human',20,.00125,'braked-5ms'],
 ['human-0p625ms','human',20,.000625,'braked-5ms'],
 ['dropout-0p625ms','dropout',12,.000625,'dropout'],
 ...(settings.include_finer_case?[['human-0p3125ms','human',20,.0003125,'braked-5ms']]:[]),
]) {
 const name=id+'-'+suffix;
 const prefix='runs/contact-planning/'+name,scene=read(source+'.scene.json'),config=read(source+'.config.json');
 const speed=settings.command_speed_m_s??scene.controller.parameters.nominal_speed_m_s;
 assert(Number.isFinite(speed)&&speed>0);
 scene.duration_s=duration;config.step_s=step;config.steps=Math.round(duration/step);config.report_every=Math.round(scene.period_s/step);
 const index=scene.controller.inputs.findIndex(ch=>ch.name==='command.forward_speed');
 assert(speed<=scene.controller.inputs[index].upper&&-speed>=scene.controller.inputs[index].lower);
 const actionsPath='examples/full-robot/fast-wasd/'+actionsName+'.actions.json';
 const actions=read(actionsPath).map(row=>{
  row[index]*=speed/.065;
  for(const [name,value] of Object.entries(settings.constant_inputs??{})) {
   const i=scene.controller.inputs.findIndex(ch=>ch.name===name);
   assert(i>=0&&Number.isFinite(value)&&value>=scene.controller.inputs[i].lower&&value<=scene.controller.inputs[i].upper);
   assert(!['command.forward_speed','command.yaw_rate','command.lateral_speed','command.packet_sequence'].includes(name));
   row[i]=value;
  }
  return row;
 });
 if(actions.length!==duration/scene.period_s)throw Error('action duration mismatch');
 const row={name,kind,prefix,duration_s:duration,step_s:step,command_speed_m_s:speed,family:id,source,
  travel_heading_offset_rad:scene.controller.parameters.travel_heading_offset_rad??0,
  actions_source:actionsPath,actions_source_sha256:sha(actionsPath)};
 if(process.argv[4])row.validation_settings={path:process.argv[4],sha256:sha(process.argv[4])};
 for(const [suffix,value] of [['scene',scene],['config',config],['actions',actions]]) {
  const path=prefix+'.'+suffix+'.json';fs.writeFileSync(path,JSON.stringify(value)+'\n',{flag:'wx'});row[suffix+'_sha256']=sha(path);
 }
 rows.push(row);
}
fs.writeFileSync(d+'/'+(process.argv[3]?id+'-':'')+'validation-cases.json',JSON.stringify({version:1,rows},null,2)+'\n',{flag:'wx'});
