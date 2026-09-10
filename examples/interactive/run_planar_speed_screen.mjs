// Orchestration: shared Rust transforms, runtime and planar predictor own all
// trajectory algebra, physics and fitted motion. Predicted scores are proposals.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {loadExperiment, materialize} from './run_affine_speed_search.mjs';
const read=p=>JSON.parse(fs.readFileSync(p));
const write=(p,x)=>fs.writeFileSync(p,JSON.stringify(x)+'\n',{flag:'wx'});
const pin=path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
async function execute(root,name,binary,args,stdoutName=name+'.stdout.log') {
  const out=fs.openSync(root+'/'+stdoutName,'wx'),err=fs.openSync(root+'/'+name+'.stderr.log','wx');
  const start=Date.now();
  const result=await new Promise(resolve=>{
    const child=spawn(binary,args,{stdio:['ignore',out,err],env:{...process.env,OMP_NUM_THREADS:'1',VECLIB_MAXIMUM_THREADS:'1',RAYON_NUM_THREADS:'2',EGOBOX_LOG:'off'}});
    write(root+'/'+name+'.launch.json',{binary,args,pid:child.pid??null,started_utc:new Date(start).toISOString()});
    child.once('error',e=>resolve({exit_code:null,signal:null,error:e.message}));
    child.once('close',(exit_code,signal)=>resolve({exit_code,signal,error:null}));
  });
  fs.closeSync(out);fs.closeSync(err);
  write(root+'/'+name+'.execution.json',{...result,wall_s:(Date.now()-start)/1000});return result;
}
const specPath=process.argv[2];assert(specPath,'usage: run_planar_speed_screen specification.json');
const spec=read(specPath),sourceSpec=read(spec.source_experiment),source=loadExperiment(sourceSpec),root=spec.output_directory;
assert.equal(spec.version,1);assert(Number.isSafeInteger(spec.seed)&&spec.seed>=0);
assert(Number.isInteger(spec.design_count)&&spec.design_count>0);
assert(Number.isInteger(spec.parallel)&&spec.parallel>=1&&spec.parallel<=3);
assert(spec.prefix_s>0&&spec.prefix_s<source.duration);
const stride=source.task.period_s/source.config.step_s,prefixSteps=spec.prefix_s/source.config.step_s;
assert(Number.isInteger(stride)&&Number.isInteger(prefixSteps)&&prefixSteps%stride===0);
assert(spec.fit_windows.length>0&&spec.fit_windows.every(([a,b])=>a>=0&&a<b&&b===spec.prefix_s));
const template=read(spec.template_capture).recording;
assert.equal(template.kind,'embedded_session');assert.equal(template.version,3);
fs.mkdirSync(root);write(root+'/spec.json',spec);
const context={version:1,files:[specPath,spec.source_experiment,spec.template_capture,spec.runtime,spec.predictor,sourceSpec.affine_binary,sourceSpec.selector,sourceSpec.scene,sourceSpec.config,sourceSpec.task,sourceSpec.actions,import.meta.filename,'examples/interactive/run_affine_speed_search.mjs'].map(pin),
  scope:'Short physical input replays plus past-only constant-twist prediction of full-horizon net speed. Predicted ranking selects future experiments; it cannot qualify speed or survival. Full-horizon searches continue independently. Seeded design bounds are exploratory domains, not physical maximum claims.'};
write(root+'/inputs.json',context);
const rows=[];
async function evaluate(values,ordinal) {
  const path=root+'/evaluation-'+String(ordinal).padStart(3,'0');
  materialize(sourceSpec,source,values,path);
  const runtime=structuredClone(template);runtime.scene=read(path+'/scene.json');runtime.config=source.config;runtime.seed=spec.seed;runtime.completed_steps=prefixSteps;
  runtime.input_events=read(path+'/actions.json').slice(0,prefixSteps/stride).map((values,i)=>({at_step:i*stride,values}));
  write(path+'/replay-input.json',{version:1,kind:'sampled_environment_recording',task:source.task,runtime,error:null});
  write(path+'/screen-inputs.json',{version:1,files:[path+'/replay-input.json',spec.runtime,spec.predictor].map(pin),scope:'Authored replay recipe, not a previously observed recording. Original full episode remains declared; capture requests only a prefix.'});
  const execution=await execute(path,'capture',spec.runtime,['--replay',path+'/replay-input.json'],'native.json');
  let capture=null;try {capture=read(path+'/native.json');} catch {}
  const last=capture?.transitions?.at(-1);
  const valid=execution.exit_code===0&&capture?.error===null&&capture.requested_steps_completed&&last?.time_s===spec.prefix_s&&!last.terminated&&!last.speed.fallen;
  if(!valid) {
    const row={ordinal,values,status:'failed',elapsed_s:last?.time_s??null,fallen:last?.speed?.fallen??null,error:capture?.error??execution.error??'prefix did not complete',predicted_speed_m_s:null};
    assert(ordinal!==0,'baseline prefix must complete');rows[ordinal]=row;write(path+'/summary.json',row);console.log(JSON.stringify(row));return;
  }
  if(ordinal===0)assert(Math.abs(last.speed.net_distance_m-spec.baseline_distance_m)<=spec.baseline_tolerance_m,'baseline prefix distance mismatch');
  const poses=capture.frames.map(f=>{
    const p=f.poses.find(p=>p.name===spec.body_link);assert(p,'declared body missing');
    return {time_s:f.time_s,pose:[...p.position_m.slice(0,2),Math.atan2(p.rotation[1][0],p.rotation[0][0])]};
  });
  const forecasts=[];
  for(let i=0;i<spec.fit_windows.length;i++) {
    const [start,end]=spec.fit_windows[i],request=path+`/fit-${i}.request.json`,output=path+`/fit-${i}.result.json`;
    write(request,{samples:poses.filter(p=>p.time_s>=start&&p.time_s<=end),origin:poses[0],queries:[{time_s:source.duration}]});
    const fit=await execute(path,'fit-'+i,spec.predictor,[request,output]);assert.equal(fit.exit_code,0,'prediction failed');
    forecasts.push(read(output));
  }
  const speeds=forecasts.map(f=>f.forecasts[0].predicted_net_speed_m_s);
  const row={ordinal,values,status:'predicted',elapsed_s:spec.prefix_s,fallen:false,prefix_net_speed_m_s:last.speed.net_distance_m/spec.prefix_s,
    predicted_speed_m_s:Math.max(...speeds),fit_window_speeds_m_s:speeds,
    scope:'Optimistic maximum across declared fitting windows, not a calibrated confidence bound. Used only to prioritize full simulations.'};
  rows[ordinal]=row;write(path+'/summary.json',row);console.log(JSON.stringify(row));
}
await evaluate(sourceSpec.baseline_values,0);
const contextId=crypto.createHash('sha256').update(JSON.stringify(context)).digest('hex');
write(root+'/design.request.json',{problem:{...sourceSpec.problem,context_id:contextId},count:spec.design_count,seed:spec.seed});
assert.equal((await execute(root,'design',sourceSpec.selector,['--design',root+'/design.request.json',root+'/design.json'])).exit_code,0);
const design=read(root+'/design.json').values;
for(let start=0;start<design.length&&!fs.existsSync(root+'/STOP');start+=spec.parallel) {
  await Promise.all(design.slice(start,start+spec.parallel).map((v,i)=>evaluate(v,start+i+1)));
  write(root+`/batch-${start}.json`,rows);
}
const ranked=rows.filter(r=>r.status==='predicted').sort((a,b)=>b.predicted_speed_m_s-a.predicted_speed_m_s);
write(root+'/result.json',{version:1,rows,ranked,stopped:fs.existsSync(root+'/STOP'),scope:context.scope});
