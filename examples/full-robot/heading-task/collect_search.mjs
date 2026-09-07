// Preserve every trial and materialize the selected policy without changing gates.
import {readFileSync,writeFileSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [run,output='examples/full-robot/heading-task']=process.argv.slice(2);assert(run,'usage: collect_search.mjs completed-search-directory [output-directory]');
const read=p=>JSON.parse(readFileSync(p));
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const write=(name,data)=>writeFileSync(`${output}/${name}`,JSON.stringify(data,null,2)+'\n');
const result=read(`${run}/search.json`),recipe=read(`${run}/recipe.json`);
assert.deepEqual(recipe,read(`${output}/search.recipe.json`));
assert.equal(result.trials.length,2*recipe.search.iterations);
const evaluations=[];
for(let i=0;i<=result.trials.length;i++){
 const trial=read(`${run}/evaluation-${String(i).padStart(4,'0')}.json`);
 const cases=recipe.cases.map((c,j)=>{
  const path=`${run}/evaluation-${String(i).padStart(4,'0')}-case-${String(j).padStart(2,'0')}.json`;
  const report=read(path).report,w=report.final_transition?.walking;
  return {name:c.name,score:report.score,error:report.error,completed_actions:report.completed_actions,
   final_heading_rad:w?.heading?.error_rad,final_body_error_m:w?Math.hypot(...w.body_error_world_m):null,
   qualified_steps:w?.qualified_steps,failed_steps:w?.failed_steps,report_sha256:hash(path)};
 });
 evaluations.push({evaluation:i,score:trial.score,error:trial.error,cases});
}
const policy=read(`${run}/policy.json`);assert.deepEqual(policy,result.policy);
write('search-result.json',result);
for(const [j,c] of recipe.cases.entries()){
 const name=c.name;assert(/^[A-Za-z0-9_.-]+$/.test(name)&&name!=='.'&&name!=='..');
 const config=read(`${run}/case-${String(j).padStart(2,'0')}.config.json`);
 assert.deepEqual(config.policy.neural_residual,policy);
 write(`${name}.config.json`,config);
}
const validationPath=`${output}/validation-plan.json`;
const reserved=existsSync(validationPath)?read(validationPath).cases:[];
for(const c of reserved){
 const config=read(c.config);config.policy.neural_residual=policy;
 write(`${c.name}.selected.config.json`,config);
}
const inputs=[recipe.scene,...recipe.cases.flatMap(c=>[c.config,c.task,c.actions]),
 ...(existsSync(validationPath)?[validationPath]:[]),...reserved.map(c=>c.config)];
write('search-status.json',{version:1,initial_score:result.initial_score,best_score:result.best_score,
 improved:result.best_score>result.initial_score,evaluations,
 sources:[...new Set(inputs)].map(path=>({path,sha256:hash(path)})),
 resolved_sha256:hash(`${run}/resolved.json`),
 scope:'Development reward search only. Reserved disturbance acceptance, trajectory validation and browser delivery are separate requirements.'});
console.log(JSON.stringify({initial:result.initial_score,best:result.best_score,accepted:result.trials.filter(t=>t.accepted).length}));
