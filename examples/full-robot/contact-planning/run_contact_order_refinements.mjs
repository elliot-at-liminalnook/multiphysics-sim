import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
const [specPath]=process.argv.slice(2);
if(!specPath)throw Error('usage: node run_contact_order_refinements.mjs spec.json');
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
const spec=read(specPath),queue=read(spec.queue),source=read(path.join(path.dirname(spec.queue),'source.recipe.json'));
assert.equal(hash(spec.binary),spec.binary_sha256,'use the recorded, audited native implementation');
assert.equal(new Set(queue.items.map(i=>i.id)).size,queue.items.length);
assert(queue.items.every(i=>/^[a-z0-9-]+$/.test(i.id)));
fs.mkdirSync(spec.output_root);
write(path.join(spec.output_root,'spec.json'),spec);
// Spend the first local solve on the best-ranked new order; retain the control
// immediately afterward with the same allowance, then cover every other start.
const neighbors=queue.items.filter(i=>i.id!=='control');
const ordered=[...neighbors.slice(0,1),...queue.items.filter(i=>i.id==='control'),...neighbors.slice(1)];
write(path.join(spec.output_root,'queue.json'),{...queue,items:ordered});
const context={driver_sha256:hash(fileURLToPath(import.meta.url)),queue_sha256:hash(spec.queue),
  binary_sha256:hash(spec.binary),library_sha256:hash(spec.library),search_sha256:hash(spec.search),
  scene_sha256:hash(queue.scene),markers_sha256:hash(queue.markers),source_robot:source.robot};
write(path.join(spec.output_root,'context.json'),context);
const observations=[];
for(const item of ordered) {
  const prefix=path.join(spec.output_root,item.id),recipe=read(item.recipe);
  assert.deepEqual(recipe.robot,source.robot,'all physical definitions and gates must agree');
  fs.copyFileSync(item.recipe,prefix+'.recipe.json',fs.constants.COPYFILE_EXCL);
  fs.copyFileSync(spec.search,prefix+'.search.json',fs.constants.COPYFILE_EXCL);
  const args=[queue.scene,queue.markers,prefix+'.recipe.json',spec.library,prefix+'.search.json',prefix+'.checkpoints'];
  write(prefix+'.launch.json',{binary:spec.binary,args,input_sha256:hash(prefix+'.recipe.json'),search_sha256:hash(prefix+'.search.json')});
  const out=fs.openSync(prefix+'.result.json','wx'),err=fs.openSync(prefix+'.log','wx');
  const start=performance.now();
  console.error(JSON.stringify({id:item.id,status:'running',variables:recipe.variables.length}));
  let execution;
  try{execution=spawnSync(spec.binary,args,{stdio:['ignore',out,err]});}finally{fs.closeSync(out);fs.closeSync(err);}
  let result=null,error=execution.error?.message??null;
  try{result=read(prefix+'.result.json');}catch(e){error??=String(e);}
  const bestPath=prefix+'.checkpoints/best_sampled_feasible.json';
  const best=fs.existsSync(bestPath)?read(bestPath):null;
  // The child is terminal before snapshots are read. A sampled probe can be a
  // candidate, but it still needs independent dense/controller qualification.
  const report=best?.report??result?.report??null;
  const measurement=report?{sampled_feasible:report.sampled_feasible,speed_m_s:report.motion_report.speed_m_s,
    maximum_inequality:report.constraints.inequalities.reduce((a,b)=>Math.max(a,b),0),
    candidate_source:best?bestPath:prefix+'.result.json'}:null;
  const observation={id:item.id,status:result?'structured_result':'failed_execution',exit_code:execution.status,signal:execution.signal,
    wall_s:(performance.now()-start)/1000,error,measurement,
    native_status:result?.search?.native_status??null,models:result?.model_evaluations??null,
    model_budget_exhausted:result?.model_budget_exhausted??null,
    result_sha256:hash(prefix+'.result.json'),log_sha256:hash(prefix+'.log'),
    scope:'Bounded coupled local refinement. Missing measurements stay missing. A failed initializer/local solve does not exclude its contact family; sampled feasibility is not runtime gait qualification.'};
  write(prefix+'.observation.json',observation);observations.push(observation);
  console.error(JSON.stringify(observation));
}
const passing=observations.filter(o=>o.measurement?.sampled_feasible)
  .sort((a,b)=>b.measurement.speed_m_s-a.measurement.speed_m_s);
write(path.join(spec.output_root,'result.json'),{version:1,observations,best_sampled:passing[0]??null,
  unprepared_edges:queue.unprepared_edges,
  scope:'One contact-order neighborhood with a common coupled-NLP allowance per prepared start. Conic ranking only orders the queue; no prepared start is pruned. No global contact-order exhaustion or faster executed gait is established.'});
