// Controller-only smoothing; shared Rust trajectory evaluation in Rhai.
import fs from 'node:fs';
import crypto from 'node:crypto';
const directory='examples/full-robot/speed-ceiling';
const read=path=>JSON.parse(fs.readFileSync(path));
const hash=path=>crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex');
const defaultSource='runs/speed-ceiling/validation/flat-return-hip0-v125-human-fine';
const batch=process.argv[2]?read(process.argv[2]):{id:'smooth',cases:[.125,.150,.175].map(speed=>({source:defaultSource,speed,name:`smooth-hip0-v${speed*1000}-human-fine`}))};
if(!/^[a-z0-9-]+$/.test(batch.id)||!Array.isArray(batch.cases)||!batch.cases.length)throw Error('named nonempty batch required');
const reportPath=`${directory}/${batch.id}-trials.json`;
if(fs.existsSync(reportPath))throw Error('refusing overwrite batch report');
const catalog=read(`${directory}/validation-cases.json`), rows=[];
for (const def of batch.cases) {
  const {source,speed,name}=def, prefix=`runs/speed-ceiling/validation/${name}`;
  if(!/^[a-z0-9.-]+$/.test(name)||!Number.isFinite(speed)||speed<=0)throw Error('valid case name and positive speed required');
  if(fs.existsSync(`${prefix}.scene.json`))throw Error(`refusing overwrite ${name}`);
  const scene=read(`${source}.scene.json`), config=read(`${source}.config.json`);
  if(def.step_s!==undefined){
    const step=def.step_s;
    if(!Number.isFinite(step)||step<=0||Math.abs(.02/step-Math.round(.02/step))>1e-9)throw Error('step must divide controller interval');
    config.step_s=step;config.steps=Math.round(20/step);config.report_every=Math.round(.02/step);scene.duration_s=20;
  }
  const p=scene.controller.parameters, points=structuredClone(p.samples);
  const end=points.length-1;
  if(points[0].some((q,j)=>Math.abs(q-points[end][j])>1e-9))throw Error('source cycle is not closed');
  points[end]=[...points[0]];
  p.trajectory={interpolation:'periodic_cubic_b_spline',keyframes:points.map((values,i)=>({time_s:i*p.period_s/end,values}))};
  const entry=scene.controller.sources.entry;
  let code=scene.controller.sources.files[entry];
  const old='let cell=state.phase/0.02;let i=cell.floor().to_int();let blend=cell-i.to_float();';
  const target='let j=p.motor_indices[name];let target=p.samples[i][j]+blend*(p.samples[i+1][j]-p.samples[i][j]);';
  const velocity='let velocity=state.rate*(p.samples[i+1][j]-p.samples[i][j])/0.02;';
  for(const text of [old,target,velocity])if(code.split(text).length!==2)throw Error('expected unique reference law');
  code=code.replace(old,'let reference=trajectory_sample(p.trajectory,state.phase);')
    .replace(target,'let j=p.motor_indices[name];let target=reference.values[j];')
    .replace(velocity,'let velocity=state.rate*reference.rates[j];');
  scene.controller.sources.files[entry]=code;
  const channel=scene.controller.inputs.findIndex(ch=>ch.name==='command.forward_speed');
  if(channel<0)throw Error('missing speed channel');
  scene.controller.inputs[channel].lower=-speed;scene.controller.inputs[channel].upper=speed;
  const actionsSource=def.actions_source??`${source}.actions.json`, actionsSpeed=def.actions_speed??.125;
  if(!Number.isFinite(actionsSpeed)||actionsSpeed<=0)throw Error('positive source action speed required');
  const actions=read(actionsSource).map(row=>{row[channel]*=speed/actionsSpeed;return row;});
  const row={name,prefix,source,source_scene_sha256:hash(`${source}.scene.json`),actions_source:actionsSource,actions_source_sha256:hash(actionsSource),kind:'human',family:name,duration_s:20,step_s:config.step_s,command_speed_m_s:speed,
    scope:'Uniform periodic cubic B-spline control points from the existing 52 mm flat-return joint table. Shared Rust positions/derivatives; original physical model, initialization, phase/braking/lease and steering retained. Control-point hull bounds reference values, not CAD clearance. No load feedforward yet.'};
  for(const [suffix,value] of [['scene',scene],['config',config],['actions',actions]]) {
    fs.writeFileSync(`${prefix}.${suffix}.json`,JSON.stringify(value)+'\n',{flag:'wx'});
    row[`${suffix}_sha256`]=hash(`${prefix}.${suffix}.json`);
  }
  rows.push(row);catalog.rows.push(row);
}
fs.writeFileSync(reportPath,JSON.stringify({batch,rows},null,2)+'\n',{flag:'wx'});
fs.writeFileSync(`${directory}/validation-cases.json`,JSON.stringify(catalog,null,2)+'\n');
