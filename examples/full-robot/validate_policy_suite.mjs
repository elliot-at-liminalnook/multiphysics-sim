// Full captures and independent stepping/geometry checks after fixed selection.
import {readFileSync, writeFileSync, mkdirSync, openSync, closeSync} from 'node:fs';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [run, validationPath, output] = process.argv.slice(2);
assert(run && validationPath && output, 'usage: validate_policy_suite.mjs search-run validation.json NEW-output');
const read = p => JSON.parse(readFileSync(p));
const recipe = read(`${run}/recipe.json`), validation = read(validationPath), policy = read(`${run}/policy.json`);
mkdirSync(output);
const execute = (cmd, args, log) => new Promise((resolve,reject) => {
  const fd = openSync(log,'w'), err = openSync(log+'.stderr.log','w'), child = spawn(cmd,args,{stdio:['ignore',fd,err]});
  child.once('error',e=>{closeSync(fd);closeSync(err);reject(e);});
  child.once('exit',code=>{closeSync(fd);closeSync(err);resolve(code);});
});
const cases = [
  ...recipe.cases.map((c,i)=>({...c,scene:recipe.scene,config:`${run}/case-${String(i).padStart(2,'0')}.config.json`, role:'development'})),
  {...validation,name:'heldout-baseline',role:'validation baseline'},
  {...validation,name:'heldout-selected',role:'validation selected'},
];
const results = [];
for (const [index,c] of cases.entries()) {
  const prefix = `${output}/case-${index}`, config = read(c.config);
  if(c.name==='heldout-selected')config.policy.neural_residual=policy;
  writeFileSync(`${prefix}.config.json`,JSON.stringify(config));
  const capturePath=`${prefix}.native.json`;
  const exit = await execute('target/release/examples/run_environment',[c.scene,`${prefix}.config.json`,c.task,c.actions],capturePath);
  assert.equal(exit,0,'capture tool failed; inspect '+capturePath);
  const capture = read(capturePath);
  let acceptance=null;
  if(capture.completed&&!capture.error){
    // A failed acceptance assertion writes its report first. Keep that evidence.
    await execute(process.execPath,['examples/full-robot/check_online_steps.mjs',capturePath,`${prefix}-acceptance`],`${prefix}-acceptance.log`);
    acceptance=read(`${prefix}-acceptance/summary.json`);
  }
  const t=capture.transitions.at(-1);
  if(c.role==='development'){
    const search=read(`${run}/search.json`);
    const lastAccepted=search.trials.reduce((last,t,i)=>t.accepted?i+1:last,0);
    const original=read(`${run}/evaluation-${String(lastAccepted).padStart(4,'0')}-case-${String(index).padStart(2,'0')}.json`).report;
    assert(Math.abs(capture.transitions.reduce((s,t)=>s+t.reward,0)-original.score)<1e-8,'selected candidate did not reproduce its score');
    assert.deepEqual(t.walking,original.final_transition.walking);
  }
  const result={name:c.name,role:c.role,completed:capture.completed,error:capture.error,
    reward:capture.completed&&!capture.error?capture.transitions.reduce((s,t)=>s+t.reward,0):null,
    passed:acceptance?.passed??false,acceptance,
    capture_sha256:createHash('sha256').update(readFileSync(capturePath)).digest('hex')};
  results.push(result);
  console.log(JSON.stringify({name:c.name,passed:result.passed,qualified:t?.walking?.qualified_steps,failed:t?.walking?.failed_steps,body_error_m:acceptance?.final_body_error_m}));
  writeFileSync(`${output}/validation-status.json`,JSON.stringify({version:1,complete:results.length===cases.length,results},null,2)+'\n');
}
