// Re-execute the selected weights; reward selection does not imply acceptance.
import {readFileSync,writeFileSync,mkdirSync,openSync,closeSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [run,suitePath,reportPath]=process.argv.slice(2);assert(run,'usage: validate_selected.mjs NEW-output-directory [suite.json report.json]');
assert(Boolean(suitePath)===Boolean(reportPath),'a custom suite requires an explicit report path');
mkdirSync(run);const root='examples/full-robot/heading-task';
const read=p=>JSON.parse(readFileSync(p)),hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const recipe=read(suitePath??`${root}/search.recipe.json`),reserved=read(`${root}/validation-plan.json`);
assert.equal(recipe.version,1);assert(recipe.cases.length>0);
const cases=suitePath?recipe.cases.map(c=>({...c,task:c.task??recipe.task,reserved:c.reserved??false})):[...recipe.cases.map(c=>({...c,config:`${root}/${c.name}.config.json`,reserved:false})),
 ...reserved.cases.map(c=>({...c,config:`${root}/${c.name}.selected.config.json`,task:`${root}/task.json`,reserved:true}))];
const reports=[];
for(const c of cases){
 const capture=`${run}/${c.name}.native.json`,log=`${run}/${c.name}.log`,fd=openSync(capture,'w'),err=openSync(log,'w');
 const execution=spawnSync('target/release/examples/run_environment',[recipe.scene,c.config,c.task,c.actions],{stdio:['ignore',fd,err]});closeSync(fd);closeSync(err);
 assert(!execution.error,execution.error?.message);
 let acceptance=null;
 if(execution.status===0){
  const fd=openSync(`${run}/${c.name}-acceptance.log`,'w');
  const check=spawnSync(process.execPath,['examples/full-robot/check_online_steps.mjs',capture,`${run}/${c.name}-acceptance`],{stdio:['ignore',fd,fd]});closeSync(fd);
  assert(!check.error,check.error?.message);
  acceptance=read(`${run}/${c.name}-acceptance/summary.json`);
 }
 const recorded=read(capture),w=recorded.transitions.at(-1)?.walking;
 const report={name:c.name,reserved:c.reserved,completed:recorded.completed,error:recorded.error,
  passed:acceptance?.passed??false,acceptance,
  total_reward:recorded.transitions.reduce((n,t)=>n+t.reward,0),final_heading:w?.heading,
  sources:[recipe.scene,c.config,c.task,c.actions,capture].map(path=>({path,sha256:hash(path)}))};
 reports.push(report);console.log(JSON.stringify({name:c.name,passed:report.passed,error:report.error,heading:w?.heading?.error_rad}));
 writeFileSync(reportPath??`${root}/validation-status.json`,JSON.stringify({version:1,complete:reports.length===cases.length,
  passed:reports.length===cases.length&&reports.every(r=>r.passed),cases:reports,
  scope:'Sampled whole-trajectory support/geometry and endpoint stopping checks at unchanged thresholds. Reserved cases excluded from checkpoint selection. Not hardware accuracy, broad terrain or browser performance.'},null,2)+'\n');
}
