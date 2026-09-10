// Configuration and experiment orchestration. All physics, force solves,
// trajectory optimization and adaptive selection use shared Rust components.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
const [specPath]=process.argv.slice(2);assert(specPath,'pass mixed search spec');
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
const spec=read(specPath),root=spec.output_root;fs.mkdirSync(root);
write(root+'/spec.json',spec);
for(const tool of Object.values(spec.tools))assert.equal(hash(tool.path),tool.sha256);
function run(prefix,binary,args) {
  const out=fs.openSync(prefix+'.stdout.json','wx'),err=fs.openSync(prefix+'.stderr.log','wx');
  const started=performance.now();let execution;
  try {execution=spawnSync(binary,args,{stdio:['ignore',out,err],env:{...process.env,OMP_NUM_THREADS:'1',VECLIB_MAXIMUM_THREADS:'1'}});}
  finally {fs.closeSync(out);fs.closeSync(err);}
  const record={binary,args,exit_code:execution.status,signal:execution.signal,
    error:execution.error?.message??null,wall_s:(performance.now()-started)/1000};
  write(prefix+'.execution.json',record);return record;
}
function select(name,request) {
  const prefix=root+'/'+name;write(prefix+'.request.json',request);
  const execution=run(prefix,spec.tools.selector.path,[prefix+'.request.json',prefix+'.response.json']);
  assert.equal(execution.exit_code,0,'selector failed; keep request, state and logs');
  return read(prefix+'.response.json');
}
const templates=[];
for(let order=0;order<spec.orders.length;order++)for(let repeat=0;repeat<spec.repetitions.length;repeat++) {
  const source=read(spec.orders[order].recipe),count=spec.repetitions[repeat];
  assert(Number.isInteger(count)&&count>=1);
  const prefix=root+`/template-${order}-${repeat}`;
  const execution=run(prefix,spec.tools.expander.path,[spec.orders[order].recipe,String(count)]);
  assert.equal(execution.exit_code,0);
  const recipe=read(prefix+'.stdout.json');
  assert.equal(recipe.robot.uniform_samples,source.robot.uniform_samples*count);
  assert.deepEqual(recipe.robot.additional_phases,Array.from({length:count},(_,r)=>source.robot.additional_phases.map(p=>(p+r)/count)).flat());
  assert.deepEqual({...recipe.robot,uniform_samples:source.robot.uniform_samples,additional_phases:source.robot.additional_phases},source.robot);
  templates.push({order,repeat,path:prefix+'.stdout.json',sha256:hash(prefix+'.stdout.json')});
}
const context={spec,driver_sha256:hash(import.meta.filename),templates,
  scene_sha256:hash(spec.scene),markers_sha256:hash(spec.markers),
  search_sha256:hash(spec.search),library_sha256:hash(spec.library),
  sources:spec.orders.map(o=>({...o,sha256:hash(o.recipe)}))};
const contextId=crypto.createHash('sha256').update(JSON.stringify(context)).digest('hex');
write(root+'/context.json',{...context,context_id:contextId});
const problem={continuous:{context_id:contextId,parameters:[
  {name:'period_box_fraction',unit:'1',bounds:[0,1]},
  {name:'displacement_box_fraction',unit:'1',bounds:[0,1]}],
  objective_name:'negative sampled planning speed after coupled local refinement',objective_unit:'m/s',
  constraints:[{name:'maximum signed planning inequality',unit:'1',scale:1}]},
  categories:[{name:'contact_order',labels:spec.orders.map(o=>o.id)},
    {name:'cycle_repetitions',labels:spec.repetitions.map(String)}]};
let state=select('initialize',{operation:'initialize',problem,config:spec.config});
// Explicit warm distribution in normalized initializer coordinates. Original
// optimization boxes and physical gates remain unchanged.
state.means=spec.initial_means;state.stds=spec.initial_stds;
write(root+'/initial-state.json',state);
const all=[];
for(let generation=0;generation<spec.generations;generation++) {
  const batch=select(`generation-${generation}-ask`,{operation:'ask',problem,config:spec.config,state});
  const observations=[];
  for(let i=0;i<batch.points.length;i++) {
    const point=batch.points[i],prefix=root+`/g${generation}-trial-${i}`;
    const template=templates.find(t=>t.order===point.categories[0]&&t.repeat===point.categories[1]);
    const recipe=read(template.path);
    const bound=kind=>recipe.variables.find(v=>v.decision.kind==='motion'&&v.decision.decision.kind===kind).bound;
    const value=(b,x)=>b.lower+x*(b.upper-b.lower);
    recipe.candidate.motion.period_s=value(bound('period'),point.continuous[0]);
    const distance=value(bound('displacement_along_direction'),point.continuous[1]);
    recipe.candidate.motion.displacement_world_m=recipe.robot.direction_world.map(d=>d*distance);
    write(prefix+'.initializer.json',recipe);
    console.error(JSON.stringify({generation,trial:i,point,template:template.path,status:'running'}));
    const started=performance.now();
    const conicExecution=run(prefix+'-conic',spec.tools.conic.path,[spec.scene,spec.markers,prefix+'.initializer.json']);
    let conic=null;
    try{conic=read(prefix+'-conic.stdout.json');}catch{}
    // All starts still receive the coupled solve, even after conic failure.
    const useConic=!!conic?.candidate&&conic.force_box_violation_n===0;
    if(useConic)recipe.candidate=conic.candidate;
    write(prefix+'.recipe.json',recipe);
    const execution=run(prefix+'-joint',spec.tools.optimizer.path,[spec.scene,spec.markers,prefix+'.recipe.json',spec.library,spec.search,prefix+'.checkpoints']);
    let result=null;try{result=read(prefix+'-joint.stdout.json');}catch{}
    const bestPath=prefix+'.checkpoints/best_sampled_feasible.json';
    const best=fs.existsSync(bestPath)?read(bestPath):null;
    const report=best?.report??result?.report;
    let outcome;
    if(report) {
      const violation=report.constraints.inequalities.reduce((m,v)=>Math.max(m,v),0);
      assert.equal(report.sampled_feasible,violation<=0);
      outcome={status:'complete',objective:-report.motion_report.speed_m_s,residuals:[violation]};
    } else outcome={status:'failed',reason:execution.error??result?.returned_candidate_error??'no auditable planning report; inspect retained execution and solver logs'};
    const observation={point,outcome,evidence:prefix+'.observation.json'};
    write(observation.evidence,{observation,template,conic_execution:conicExecution,conic_warm_start_used:useConic,execution,
      model_evaluations:result?.model_evaluations??null,native_status:result?.search?.native_status??null,
      model_budget_exhausted:result?.model_budget_exhausted??null,
      wall_s:(performance.now()-started)/1000,
      candidate_source:best?bestPath:report?prefix+'-joint.stdout.json':null,
      scope:'Sampled planning only. Failed local solves do not exclude families. Runtime, dense geometry, timestep and control validation remain required.'});
    observations.push(observation);all.push(observation);
    console.error(JSON.stringify({generation,trial:i,outcome}));
  }
  const update=select(`generation-${generation}-tell`,{operation:'tell',problem,config:spec.config,batch,observations});
  state=update.state;
}
write(root+'/result.json',{problem,state,observations:all,
  scope:'Adaptive mixed selection of existing contact orders, one/two repeated-cycle seeds, period and displacement initializers. Inner solve jointly optimizes the trajectory. This is a bounded planning pilot, not full CrEGOpt, all contact sequences, runtime success, or physical maximum.'});
