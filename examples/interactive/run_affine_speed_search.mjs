// Configuration orchestration only: trajectory algebra, Bayesian proposals and
// every physical evaluation execute in shared Rust components.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawn, spawnSync} from 'node:child_process';
import {isDeepStrictEqual} from 'node:util';
import {pathToFileURL} from 'node:url';
const read=p=>JSON.parse(fs.readFileSync(p));
const write=(p,x)=>fs.writeFileSync(p,JSON.stringify(x)+'\n',{flag:'wx'});
const pin=path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});

export function loadExperiment(spec) {
  assert.equal(spec.version,1);
  const scene=read(spec.scene),config=read(spec.config),task=read(spec.task),actions=read(spec.actions);
  const period=task.period_s,duration=scene.duration_s;
  assert(duration>0&&Math.abs(config.step_s*config.steps-duration)<1e-8);
  assert.equal(actions.length,Math.round(duration/period));
  assert.equal(spec.cad_sha256,scene.robot.source.cad_sha256);
  assert(!config.policy.neural_residual&&!config.policy.trajectory_forecast&&!config.policy.forecast_action_search);
  const names=Object.entries(scene.controller.parameters.motor_indices).sort((a,b)=>a[1]-b[1]).map(([name])=>name);
  assert.equal(names.length,spec.centers_rad.length);
  assert(spec.centers_rad.every(Number.isFinite));
  assert.equal(new Set(spec.groups.map(g=>g.name)).size,spec.groups.length);
  assert.deepEqual(read(spec.reference),scene.controller.parameters.trajectory);
  assert.equal(scene.controller.parameters.trajectory.keyframes[0].values.length,names.length);
  const grouped=spec.groups.flatMap(g=>g.targets);
  assert.equal(new Set(grouped).size,names.length);assert.deepEqual([...grouped].sort(),[...names].sort());
  const extra=spec.extra_command_parameters??[];
  assert(Array.isArray(extra));
  const parameterNames=['command_speed','tracking_gain',...spec.groups.map(g=>g.name+'_amplitude'),...spec.groups.map(g=>g.name+'_lead'),...extra.map(b=>b.name)];
  assert.equal(new Set(parameterNames).size,parameterNames.length);
  assert.deepEqual(spec.problem.parameters.map(p=>p.name),parameterNames);
  assert.equal(spec.problem.constraints.length,1);
  const index=name=>{const i=scene.controller.inputs.findIndex(c=>c.name===name);assert(i>=0);return i;};
  const speedIndex=index('command.forward_speed'),gainIndex=index('command.tracking_gain'),sequenceIndex=index('command.packet_sequence');
  assert(actions.every(a=>a.every((v,i)=>i===sequenceIndex||v===actions[0][i])));
  assert.equal(spec.baseline_values[0],actions[0][speedIndex]);assert.equal(spec.baseline_values[1],actions[0][gainIndex]);
  const extraOffset=2+2*spec.groups.length;
  assert.equal(spec.baseline_values.length,parameterNames.length);
  assert(spec.baseline_values.slice(2,extraOffset).every(x=>x===1));
  if(extra.length)assert(typeof spec.command_schema==='string','extra commands require exported channel schema');
  const schema=extra.length?read(spec.command_schema):[];
  const extraCommands=extra.map((binding,j)=>{
    const inputIndex=index(binding.input),parameterIndex=extraOffset+j;
    assert(![speedIndex,gainIndex,sequenceIndex].includes(inputIndex),'extra commands must use distinct non-heartbeat inputs');
    const matches=schema.filter(c=>c.name===binding.input);
    assert.equal(matches.length,1,'one exported channel schema required');
    assert.equal(matches[0].kind,scene.controller.inputs[inputIndex].kind,'command quantity kind mismatch');
    assert.equal(matches[0].unit,spec.problem.parameters[parameterIndex].unit,'command parameter unit mismatch');
    assert(typeof matches[0].unit==='string'&&matches[0].unit.length>0);
    assert.equal(spec.baseline_values[parameterIndex],actions[0][inputIndex]);
    return {inputIndex,parameterIndex};
  });
  assert.equal(new Set(extraCommands.map(b=>b.inputIndex)).size,extraCommands.length,'duplicate extra command binding');
  return {scene,config,task,actions,names,speedIndex,gainIndex,duration,extraCommands};
}

export function materialize(spec,source,values,root) {
  assert.equal(values.length,spec.problem.parameters.length);
  assert(values.every((v,i)=>Number.isFinite(v)&&v>=spec.problem.parameters[i].bounds[0]&&v<=spec.problem.parameters[i].bounds[1]));
  fs.mkdirSync(root);
  const s=structuredClone(source.scene),actions=structuredClone(source.actions),n=spec.groups.length;
  const scales=source.names.map(name=>values[2+spec.groups.findIndex(g=>g.targets.includes(name))]);
  write(root+'/transform.json',{scales,centers:spec.centers_rad});
  const transform=spawnSync(spec.affine_binary,[spec.reference,root+'/transform.json',root+'/trajectory.json'],{encoding:'utf8'});
  write(root+'/transform-execution.json',{exit_code:transform.status,signal:transform.signal,error:transform.error?.message??null,stderr:transform.stderr});
  assert.equal(transform.status,0,transform.stderr);
  s.controller.parameters.trajectory=read(root+'/trajectory.json');
  s.controller.parameters.velocity_lead_s=source.names.map((name,j)=>source.scene.controller.parameters.velocity_lead_s[j]*values[2+n+spec.groups.findIndex(g=>g.targets.includes(name))]);
  const [speed,gain]=values;
  Object.assign(s.controller.inputs[source.speedIndex],{lower:spec.problem.parameters[0].bounds[0],upper:spec.problem.parameters[0].bounds[1]});
  Object.assign(s.controller.inputs[source.gainIndex],{lower:spec.problem.parameters[1].bounds[0],upper:spec.problem.parameters[1].bounds[1],initial:gain});
  for(const a of actions){a[source.speedIndex]=speed;a[source.gainIndex]=gain;}
  for(const {inputIndex,parameterIndex} of source.extraCommands){
    const [lower,upper]=spec.problem.parameters[parameterIndex].bounds;
    const channel=s.controller.inputs[inputIndex];
    Object.assign(channel,{lower,upper});
    if(channel.initial<lower||channel.initial>upper)channel.initial=values[parameterIndex];
    for(const action of actions)action[inputIndex]=values[parameterIndex];
  }
  const restored=structuredClone(s);restored.controller.inputs=source.scene.controller.inputs;
  restored.controller.parameters.trajectory=source.scene.controller.parameters.trajectory;
  restored.controller.parameters.velocity_lead_s=source.scene.controller.parameters.velocity_lead_s;
  assert(isDeepStrictEqual(restored,source.scene),'unexpected robot/world/policy mutation');
  write(root+'/scene.json',s);write(root+'/actions.json',actions);write(root+'/values.json',values);
  write(root+'/inputs.json',{version:1,files:[root+'/scene.json',root+'/actions.json',root+'/trajectory.json',root+'/transform.json',spec.config,spec.task,spec.evaluator,...(spec.command_schema?[spec.command_schema]:[])].map(pin)});
}

async function execute(root,name,binary,args) {
  const out=fs.openSync(root+'/'+name+'.stdout.log','wx'),err=fs.openSync(root+'/'+name+'.stderr.log','wx');
  const started=Date.now();
  const result=await new Promise(resolve=>{
    const child=spawn(binary,args,{stdio:['ignore',out,err],env:{...process.env,OMP_NUM_THREADS:'1',VECLIB_MAXIMUM_THREADS:'1',RAYON_NUM_THREADS:'2',EGOBOX_LOG:'off'}});
    write(root+'/'+name+'.launch.json',{binary,args,pid:child.pid??null,started_utc:new Date(started).toISOString()});
    child.once('error',error=>resolve({exit_code:null,signal:null,error:error.message}));
    child.once('close',(exit_code,signal)=>resolve({exit_code,signal,error:null}));
  });
  fs.closeSync(out);fs.closeSync(err);
  write(root+'/'+name+'.execution.json',{...result,wall_s:(Date.now()-started)/1000});
  return result;
}

async function main(specPath) {
  const spec=read(specPath),source=loadExperiment(spec),root=spec.output_directory;
  assert(Number.isInteger(spec.initial_design_count)&&spec.initial_design_count>=1);
  assert(Number.isInteger(spec.adaptive_evaluations)&&spec.adaptive_evaluations>=1);
  assert(Number.isInteger(spec.parallel_initial)&&spec.parallel_initial>=1&&spec.parallel_initial<=3);
  fs.mkdirSync(root);
  const context={version:1,files:[specPath,spec.scene,spec.config,spec.task,spec.actions,spec.reference,spec.affine_binary,spec.selector,spec.evaluator,import.meta.filename,...(spec.command_schema?[spec.command_schema]:[])].map(pin),
    problem:spec.problem,groups:spec.groups,centers_rad:spec.centers_rad,seed:spec.seed,duration_s:source.duration,
    scope:'Full-horizon net distance/time without sampled falls. Authored amplitudes and lead factors are independent numerical search variables. Existing CAD, actuator physics and output bounds remain unchanged. This finite search is not gait-space exhaustion.'};
  const contextId=crypto.createHash('sha256').update(JSON.stringify(context)).digest('hex'),problem={...spec.problem,context_id:contextId};
  write(root+'/context.json',{...context,context_id:contextId});write(root+'/spec.json',spec);
  const observations=[];let next=0;
  const stopped=()=>fs.existsSync(root+'/STOP');
  async function evaluate(values,method) {
    const ordinal=next++,name='evaluation-'+String(ordinal).padStart(3,'0'),path=root+'/'+name;
    materialize(spec,source,values,path);
    const result=await execute(path,'evaluate',spec.evaluator,[path+'/scene.json',spec.config,spec.task,path+'/actions.json',path+'/result.json',String(spec.seed)]);
    const report=fs.existsSync(path+'/result.json')?read(path+'/result.json'):null,t=report?.final_transition;
    const complete=result.exit_code===0&&report?.error===null&&report.score!==null&&t?.time_s===source.duration&&t.truncated&&!t.terminated&&!t.speed.fallen;
    const outcome=complete?{status:'complete',objective:-t.speed.net_speed_m_s,residuals:[-1]}:
      {status:'failed',reason:report?.error??result.error??'incomplete or abnormal evaluation'};
    if(ordinal===0){assert(complete,'baseline must finish before adaptive search');assert(Math.abs(report.score-spec.baseline_expectation.score)<=spec.baseline_expectation.absolute_tolerance,'baseline physical score mismatch');}
    const row={context_id:contextId,values,outcome,evidence:path+'/observation.json'};
    write(row.evidence,{observation:row,method,ordinal,physical_result:report?pin(path+'/result.json'):null,
      sampled_fall:t?.speed?.fallen??null,elapsed_s:t?.time_s??null,
      scope:'Only complete nonfalling episodes supply speed scores. A physical fall or numerical error remains a failed outcome, never an invented speed reward; failed rows are preserved but excluded by the current Bayesian backend.'});
    observations[ordinal]=row;
    console.log(JSON.stringify({name,values,outcome}));
  }
  await evaluate(spec.baseline_values,'baseline');
  write(root+'/initial.request.json',{problem,count:spec.initial_design_count,seed:spec.seed});
  assert.equal((await execute(root,'initial-design',spec.selector,['--design',root+'/initial.request.json',root+'/initial.design.json'])).exit_code,0);
  const design=read(root+'/initial.design.json').values;
  for(let start=0;start<design.length&&!stopped();start+=spec.parallel_initial){
    await Promise.all(design.slice(start,start+spec.parallel_initial).map(values=>evaluate(values,'seeded-latin-hypercube')));
    write(root+`/initial-batch-${start}.observations.json`,observations);
  }
  for(let i=0;i<spec.adaptive_evaluations&&!stopped();i++){
    const request=root+`/adaptive-${i}.request.json`,output=root+`/adaptive-${i}.proposal.json`;
    if(observations.filter(o=>o.outcome.status==='complete').length<problem.parameters.length+1){
      write(request,{problem,count:1,seed:spec.seed+1000000+i});
      assert.equal((await execute(root,'adaptive-'+i,spec.selector,['--design',request,output])).exit_code,0);
      await evaluate(read(output).values[0],'seeded-design-refill-after-failed-outcomes');
      write(root+`/adaptive-${i}.observations.json`,observations);
      continue;
    }
    write(request,{problem,observations,config:{seed:spec.seed+i,acquisition_starts:8,maximum_training_rows:256}});
    assert.equal((await execute(root,'adaptive-'+i,spec.selector,[request,output])).exit_code,0);
    await evaluate(read(output).proposal.values,'constrained-log-expected-improvement');
    write(root+`/adaptive-${i}.observations.json`,observations);
  }
  const best=observations.filter(o=>o.outcome.status==='complete').sort((a,b)=>a.outcome.objective-b.outcome.objective)[0];
  write(root+'/result.json',{version:1,problem,observations,best,stopped:stopped(),duration_s:source.duration,scope:context.scope});
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
  assert(process.argv[2],'usage: run_affine_speed_search specification.json');
  await main(process.argv[2]);
}
