import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
const [casesPath,outputPrefix]=process.argv.slice(2);
assert(casesPath&&outputPrefix,'pass cases.json and fresh output prefix');
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
const cases=read(casesPath),task='examples/full-robot/fast-wasd/task.json';
write(outputPrefix+'.launch.json',{cases:casesPath,cases_sha256:hash(casesPath),task,task_sha256:hash(task),
  code:[import.meta.filename,'examples/full-robot/speed-ceiling/analyze_validation.mjs',
    'examples/full-robot/contact-planning/audit_contact_controller.mjs'].map(path=>({path,sha256:hash(path)})),
  binaries:['run_environment','replay_policy_state','evaluate_lift'].map(n=>({path:bin+'/'+n,sha256:hash(bin+'/'+n)}))});
function run(prefix,binary,args,stdoutSuffix) {
  const out=fs.openSync(prefix+stdoutSuffix,'wx'),err=fs.openSync(prefix+'.log','wx');
  const started=performance.now();let r;
  try{r=spawnSync(binary,args,{stdio:['ignore',out,err],env:{...process.env,SIM_EXAMPLES:bin,OMP_NUM_THREADS:'1',VECLIB_MAXIMUM_THREADS:'1'}});}
  finally{fs.closeSync(out);fs.closeSync(err);}
  write(prefix+'.execution.json',{binary,args,exit_code:r.status,error:r.error?.message??null,signal:r.signal,wall_s:(performance.now()-started)/1000});
  return r.status;
}
for(const row of cases.rows) {
  console.error(JSON.stringify({name:row.name,status:'running'}));
  for(const suffix of ['scene','config','actions'])assert.equal(hash(row.prefix+'.'+suffix+'.json'),row[suffix+'_sha256']);
  const code=run(row.prefix+'.native',bin+'/run_environment',[row.prefix+'.scene.json',row.prefix+'.config.json',task,row.prefix+'.actions.json'],'.json');
  const capture=read(row.prefix+'.native.json');
  if(code===0&&capture.completed) {
    const windows=row.kind==='human'?[[1.4,9.8],[11,15.8]]:row.kind==='sustained'?[[.8,20],[24.2,40],[44.2,56]]:[[.8,3],[6.8,9]];
    write(row.prefix+'.clearance.spec.json',{prefix:row.prefix,windows_s:windows});
    run(row.prefix+'.clearance','node',['examples/full-robot/contact-planning/audit_contact_controller.mjs',row.prefix+'.clearance.spec.json',row.prefix+'.clearance.summary.json'],'.stdout.json');
  }
  console.error(JSON.stringify({name:row.name,status:'terminal',completed:capture.completed,error:capture.error}));
}
assert.equal(run(outputPrefix+'.analysis','node',['examples/full-robot/speed-ceiling/analyze_validation.mjs',casesPath,outputPrefix+'.summary.json','.000625'],'.stdout.json'),0);
