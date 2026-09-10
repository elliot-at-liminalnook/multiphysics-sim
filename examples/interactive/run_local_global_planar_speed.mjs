// Stateful local/global acquisition; Rust owns all optimization and physics.
// Full-horizon measured scores never enter this estimated-objective context.
import fs from 'node:fs';import crypto from 'node:crypto';import assert from 'node:assert/strict';
import {loadExperiment} from './run_affine_speed_search.mjs';
import {execute,evaluatePlanarCandidate} from './planar_speed_experiment.mjs';
const read=p=>JSON.parse(fs.readFileSync(p)),write=(p,x)=>fs.writeFileSync(p,JSON.stringify(x)+'\n',{flag:'wx'});
const pin=path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const specPath=process.argv[2];assert(specPath,'usage: run_local_global_planar_speed specification.json');
const spec=read(specPath),seedData=read(spec.seed_results),seedContext=read(spec.seed_context),screenSpec=read(spec.screen_spec),sourceSpec=read(screenSpec.source_experiment),source=loadExperiment(sourceSpec),root=spec.output_directory;
assert.equal(spec.version,1);assert(Number.isInteger(spec.iterations)&&spec.iterations>0);assert(Number.isSafeInteger(spec.proposal_seed)&&spec.proposal_seed>=0);
const expected=path=>{const p=seedContext.files.find(p=>p.path===path);assert(p,'missing seed context pin: '+path);assert.deepEqual(pin(path),p,'seed context changed: '+path);};
expected(spec.predictor);expected(screenSpec.source_experiment);expected(spec.screen_spec);
assert.equal(seedData.problem.parameters.length,sourceSpec.problem.parameters.length);
const originalContext=read(spec.original_screen_context);for(const p of originalContext.files)assert.equal(pin(p.path).sha256,p.sha256,'original screen dependency changed: '+p.path);
const template=read(screenSpec.template_capture).recording;
fs.mkdirSync(root);write(root+'/spec.json',spec);
write(root+'/inputs.json',{version:1,files:[specPath,spec.seed_results,spec.seed_context,spec.screen_spec,spec.original_screen_context,spec.predictor,screenSpec.runtime,spec.selector,import.meta.filename,new URL('./planar_speed_experiment.mjs',import.meta.url).pathname].map(pin),scope:'Adaptive proposals from the frozen heading-trend20s prediction context. Reuses exact seed recipe, simulator seed and binary. Full300s speed and survival remain unqualified until separately simulated. STOP requests cancellation between evaluations.'});
const observations=structuredClone(seedData.observations),rows=[];let regionState=seedData.region_state??null;
for(let i=0;i<spec.iterations&&!fs.existsSync(root+'/STOP');i++){
 const proposalRoot=root+'/proposal-'+String(i).padStart(3,'0');fs.mkdirSync(proposalRoot);
 let selected,selectionMethod;
 if(observations.filter(o=>o.outcome.status==='complete').length<seedData.problem.parameters.length+1){
  assert.equal(regionState,null,'refill is only for initial model identification');
  assert(Number.isFinite(spec.refill_radius)&&spec.refill_radius>0&&spec.refill_radius<=1,'explicit normalized refill radius required');
  const best=observations.filter(o=>o.outcome.status==='complete').sort((a,b)=>a.outcome.objective-b.outcome.objective)[0];
  assert(best,'one completed seed required for local identification');
  const local=structuredClone(seedData.problem);
  local.parameters.forEach((p,j)=>{const [lo,hi]=p.bounds,r=spec.refill_radius*(hi-lo);p.bounds=[Math.max(lo,best.values[j]-r),Math.min(hi,best.values[j]+r)];});
  write(proposalRoot+'/request.json',{problem:local,count:1,seed:spec.proposal_seed+i});
  assert.equal((await execute(proposalRoot,'design-refill',sourceSpec.selector,['--design',proposalRoot+'/request.json',proposalRoot+'/result.json'])).exit_code,0);
  selected={proposal:{values:read(proposalRoot+'/result.json').values[0]}};selectionMethod='seeded local design for initial model identification';
 }else{
  write(proposalRoot+'/request.json',{problem:seedData.problem,observations,config:{seed:spec.proposal_seed+i,acquisition_starts:8,maximum_training_rows:256},region:spec.region,state:regionState});
  assert.equal((await execute(proposalRoot,'suggest',spec.selector,[proposalRoot+'/request.json',proposalRoot+'/result.json'])).exit_code,0);
  selected=read(proposalRoot+'/result.json').result;regionState=selected.state;selectionMethod='adaptive local/global acquisition';
 }
 const values=selected.proposal.values,path=root+'/evaluation-'+String(i).padStart(3,'0');
 const {row,outcome}=await evaluatePlanarCandidate({sourceSpec,source,template,screen:screenSpec,predictor:spec.predictor,path,values,ordinal:i});
 row.selection_method=selectionMethod;
 write(path+'/summary.json',row);rows.push(row);observations.push({context_id:seedData.problem.context_id,values,outcome,evidence:path+'/summary.json'});
 write(root+`/iteration-${i}.json`,{rows,observations,region_state:regionState});console.log(JSON.stringify(row));
}
write(root+'/result.json',{version:1,problem:seedData.problem,observations,rows,region_state:regionState,stopped:fs.existsSync(root+'/STOP'),scope:'Stateful local/global model-guided candidate selection. Estimated objectives are not physical speed qualifications; prefix survival is not full-horizon survival. Failed outcomes stay failed and are excluded by the current GP backend.'});
