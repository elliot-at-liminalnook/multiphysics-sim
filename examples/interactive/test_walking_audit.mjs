import {test} from 'node:test';import assert from 'node:assert/strict';
import {mkdtempSync,writeFileSync,readFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';import {join} from 'node:path';
import {createHash} from 'node:crypto';import {execFileSync} from 'node:child_process';
test('walking audit accepts omitted empty event history but rejects fabricated rejected-input events',()=>{
  const dir=mkdtempSync(join(tmpdir(),'walking-audit-'));
  const write=(name,data)=>{const p=join(dir,name);writeFileSync(p,JSON.stringify(data));return p;};
  const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
  try{
    const scene={robot:{},options:{},controller:{inputs:[{initial:0}]}},config={step_s:0.01},task={period_s:0.02};
    const error='policy inputs rejected';
    const r={completed:false,error,frames:[{time_s:0,completed_steps:0}],task,
      recording:{scene,config,seed:0,completed_steps:0}};
    const capture=write('case.native.json',r);
    const plan=write('plan.json',{sources:[],cases:[{name:'rejected',seed:0,
      scene:write('scene.json',scene),config:write('config.json',config),task:write('task.json',task),actions:write('actions.json',[[1]])}]});
    const status=()=>write('status.json',{complete:true,cases:[{name:'rejected',completed:false,error,passed:false,acceptance:null,sources:[source(capture)]}]});
    const output=join(dir,'audit.json');
    execFileSync(process.execPath,['examples/interactive/audit_walking_suite.mjs',plan,status(),output],{stdio:'pipe'});
    const audit=JSON.parse(readFileSync(output));assert(audit.passed);assert.equal(audit.outcomes[0].completed_transitions,0);
    r.recording.input_events=[{at_step:0,values:[99]}];write('case.native.json',r);
    assert.throws(()=>execFileSync(process.execPath,['examples/interactive/audit_walking_suite.mjs',plan,status(),output],{stdio:'pipe'}));
  }finally{rmSync(dir,{recursive:true,force:true});}
});
