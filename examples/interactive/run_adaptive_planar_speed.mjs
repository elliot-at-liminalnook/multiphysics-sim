// Rust owns proposals, trajectory algebra, physics and motion prediction.
// Full-horizon measured scores never enter this estimated-objective context.
import fs from 'node:fs';import crypto from 'node:crypto';import assert from 'node:assert/strict';import {spawn} from 'node:child_process';
import {loadExperiment,materialize} from './run_affine_speed_search.mjs';
const read=p=>JSON.parse(fs.readFileSync(p)),write=(p,x)=>fs.writeFileSync(p,JSON.stringify(x)+'\n',{flag:'wx'});
const pin=path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
async function execute(root,name,binary,args,stdoutName=name+'.stdout.log'){
 const out=fs.openSync(root+'/'+stdoutName,'wx'),err=fs.openSync(root+'/'+name+'.stderr.log','wx'),start=Date.now();
 const result=await new Promise(resolve=>{const child=spawn(binary,args,{stdio:['ignore',out,err],env:{...process.env,OMP_NUM_THREADS:'1',VECLIB_MAXIMUM_THREADS:'1',RAYON_NUM_THREADS:'2',EGOBOX_LOG:'off'}});
  write(root+'/'+name+'.launch.json',{binary,args,pid:child.pid??null,started_utc:new Date(start).toISOString()});child.once('error',e=>resolve({exit_code:null,signal:null,error:e.message}));child.once('close',(exit_code,signal)=>resolve({exit_code,signal,error:null}));});
 fs.closeSync(out);fs.closeSync(err);write(root+'/'+name+'.execution.json',{...result,wall_s:(Date.now()-start)/1000});return result;
}
const specPath=process.argv[2];assert(specPath,'usage: run_adaptive_planar_speed specification.json');
const spec=read(specPath),seedData=read(spec.seed_results),seedContext=read(spec.seed_context),screenSpec=read(spec.screen_spec),sourceSpec=read(screenSpec.source_experiment),source=loadExperiment(sourceSpec),root=spec.output_directory;
assert.equal(spec.version,1);assert(Number.isInteger(spec.iterations)&&spec.iterations>0);assert(Number.isSafeInteger(spec.proposal_seed)&&spec.proposal_seed>=0);
const expected=path=>{const p=seedContext.files.find(p=>p.path===path);assert(p,'missing seed context pin: '+path);assert.deepEqual(pin(path),p,'seed context changed: '+path);};
expected(spec.predictor);expected(screenSpec.source_experiment);expected(spec.screen_spec);
assert.equal(seedData.problem.parameters.length,sourceSpec.problem.parameters.length);
const originalContext=read(spec.original_screen_context);for(const p of originalContext.files)assert.equal(pin(p.path).sha256,p.sha256,'original screen dependency changed: '+p.path);
const stride=source.task.period_s/source.config.step_s,prefixSteps=screenSpec.prefix_s/source.config.step_s;
assert(Number.isInteger(stride)&&Number.isInteger(prefixSteps));
const template=read(screenSpec.template_capture).recording;
fs.mkdirSync(root);write(root+'/spec.json',spec);
write(root+'/inputs.json',{version:1,files:[specPath,spec.seed_results,spec.seed_context,spec.screen_spec,spec.original_screen_context,spec.predictor,screenSpec.runtime,sourceSpec.selector,import.meta.filename].map(pin),scope:'Adaptive proposals from the frozen heading-trend20s prediction context. Reuses exact seed recipe, simulator seed and binary. Full300s speed and survival remain unqualified until separately simulated. STOP requests cancellation between evaluations.'});
const observations=structuredClone(seedData.observations),rows=[];
for(let i=0;i<spec.iterations&&!fs.existsSync(root+'/STOP');i++){
 const proposalRoot=root+'/proposal-'+String(i).padStart(3,'0');fs.mkdirSync(proposalRoot);
 write(proposalRoot+'/request.json',{problem:seedData.problem,observations,config:{seed:spec.proposal_seed+i,acquisition_starts:8,maximum_training_rows:256}});
 assert.equal((await execute(proposalRoot,'suggest',sourceSpec.selector,[proposalRoot+'/request.json',proposalRoot+'/result.json'])).exit_code,0);
 const values=read(proposalRoot+'/result.json').proposal.values,path=root+'/evaluation-'+String(i).padStart(3,'0');materialize(sourceSpec,source,values,path);
 const runtime=structuredClone(template);runtime.scene=read(path+'/scene.json');runtime.config=source.config;runtime.seed=screenSpec.seed;runtime.completed_steps=prefixSteps;
 runtime.input_events=read(path+'/actions.json').slice(0,prefixSteps/stride).map((values,k)=>({at_step:k*stride,values}));
 write(path+'/replay-input.json',{version:1,kind:'sampled_environment_recording',task:source.task,runtime,error:null});
 write(path+'/screen-inputs.json',{version:1,files:[path+'/replay-input.json',screenSpec.runtime,spec.predictor,proposalRoot+'/result.json'].map(pin),scope:'Authored short-prefix input recipe; original full episode stays declared.'});
 const execution=await execute(path,'capture',screenSpec.runtime,['--replay',path+'/replay-input.json'],'native.json');
 let capture=null;try{capture=read(path+'/native.json');}catch{}
 const last=capture?.transitions?.at(-1),valid=execution.exit_code===0&&capture?.error===null&&capture.requested_steps_completed&&last?.time_s===screenSpec.prefix_s&&!last.terminated&&!last.speed.fallen;
 let row,outcome;
 if(!valid){
  row={ordinal:i,values,status:'failed',elapsed_s:last?.time_s??null,fallen:last?.speed?.fallen??null,error:capture?.error??execution.error??last?.termination_reasons??'prefix did not complete',predicted_speed_m_s:null};
  outcome={status:'failed',reason:typeof row.error==='string'?row.error:JSON.stringify(row.error)};
 }else{
  const poses=capture.frames.map(f=>{const p=f.poses.find(p=>p.name===screenSpec.body_link);assert(p);return {time_s:f.time_s,pose:[...p.position_m.slice(0,2),Math.atan2(p.rotation[1][0],p.rotation[0][0])]};});
  const fits=[];
  for(let j=0;j<screenSpec.fit_windows.length;j++){
   const [start,end]=screenSpec.fit_windows[j];write(path+`/fit-${j}.request.json`,{samples:poses.filter(p=>p.time_s>=start&&p.time_s<=end),origin:poses[0],queries:[{time_s:source.duration}],heading_trend:true});
   assert.equal((await execute(path,'fit-'+j,spec.predictor,[path+`/fit-${j}.request.json`,path+`/fit-${j}.result.json`])).exit_code,0);fits.push(read(path+`/fit-${j}.result.json`));
  }
  const speeds=fits.map(f=>f.forecasts[0].predicted_net_speed_m_s),predicted_speed_m_s=Math.max(...speeds);
  row={ordinal:i,values,status:'predicted',elapsed_s:screenSpec.prefix_s,fallen:false,prefix_net_speed_m_s:last.speed.net_distance_m/screenSpec.prefix_s,predicted_speed_m_s,fit_window_speeds_m_s:speeds,physical_full_horizon_qualified:false};
  outcome={status:'complete',objective:-predicted_speed_m_s,residuals:[-1]};
 }
 write(path+'/summary.json',row);rows.push(row);observations.push({context_id:seedData.problem.context_id,values,outcome,evidence:path+'/summary.json'});
 write(root+`/iteration-${i}.json`,{rows,observations});console.log(JSON.stringify(row));
}
write(root+'/result.json',{version:1,problem:seedData.problem,observations,rows,stopped:fs.existsSync(root+'/STOP'),scope:'Sequential model-guided candidate selection. Estimated objectives are not physical speed qualifications; prefix survival is not full-horizon survival. Failed outcomes stay failed and are excluded by the current GP backend.'});
