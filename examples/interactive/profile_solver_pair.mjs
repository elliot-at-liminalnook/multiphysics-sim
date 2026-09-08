// Isolated native profiles and exact physics/task preservation for a solver pair.
import {readFileSync,writeFileSync,existsSync,openSync,closeSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [studyPath,statusPath,output,previousPath]=process.argv.slice(2);assert(output,'usage: profile_solver_pair study.json status.json report.json [previous-reference-capture]');
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const stable=x=>JSON.stringify(x,(k,v)=>k==='stepping_wall_s'?undefined:v);
const p95=a=>[...a].sort((a,b)=>a-b)[Math.ceil(.95*a.length)-1];
const study=read(studyPath),plan=read(study.plan_output),status=read(statusPath);assert(status.complete&&plan.cases.length===2);
const executable=source('target/release/examples/run_environment');
const originals=plan.cases.map(c=>read(`${study.output}/${c.name}.native.json`));
assert(originals.every(r=>r.completed&&!r.error));
const cleanRecord=r=>{r=structuredClone(r);const keys=study.boolean_solver_option.split('.'),field=keys.pop();let c=r.config;for(const key of keys)c=c[key];delete c[field];return r;};
assert(stable(cleanRecord(originals[0].recording))===stable(cleanRecord(originals[1].recording)),'only declared solver option may differ');
assert(stable(originals[0].task)===stable(originals[1].task)&&stable(originals[0].contract)===stable(originals[1].contract));
const pair_frames_exact=stable(originals[0].frames)===stable(originals[1].frames);
const pair_transitions_exact=stable(originals[0].transitions)===stable(originals[1].transitions);
let previous_reference_exact=null;
if(previousPath){const old=read(previousPath);previous_reference_exact=['frames','transitions','recording','task','contract'].every(k=>stable(old[k])===stable(originals[0][k]));}
const cases=[];
for(const [index,c] of plan.cases.entries()){
  const capture=`${study.output}/${c.name}.profiled.native.json`,profile=`${study.output}/${c.name}.profile.json`,log=`${study.output}/${c.name}.profile.log`;
  assert(!existsSync(capture)&&!existsSync(profile)&&!existsSync(log));assert.equal(source(executable.path).sha256,executable.sha256);
  const out=openSync(capture,'wx'),err=openSync(log,'wx');
  const execution=spawnSync(executable.path,[c.scene,c.config,c.task,c.actions,'--profile',profile],{stdio:['ignore',out,err]});closeSync(out);closeSync(err);
  assert(!execution.error&&execution.status===0);assert.equal(source(executable.path).sha256,executable.sha256);
  const measured=read(capture),original=originals[index],p=read(profile);assert(measured.completed&&p.completed);
  for(const key of ['frames','transitions','recording','task','contract'])assert(stable(measured[key])===stable(original[key]),`profiling changes ${key}`);
  const samples=original.frames.slice(1).map((f,i)=>({phase:f.policy?.step_reference?.reference.phase??'unknown',wall_s:original.transition_wall_s[i],step_clock_s:f.stepping_wall_s-original.frames[i].stepping_wall_s}));
  const active=samples.filter(s=>!['hold','idle'].includes(s.phase));assert(samples.every(s=>Number.isFinite(s.wall_s)&&s.wall_s>=0));
  cases.push({name:c.name,profile_preserves_frames_and_transitions:true,unprofiled_wall_s:original.wall_s,profile_wall_s:p.wall_s,
    native_transition_p95_s:p95(samples.map(s=>s.wall_s)),native_active_transition_p95_s:p95(active.map(s=>s.wall_s)),
    native_active_step_clock_p95_s:p95(active.map(s=>s.step_clock_s)),buckets:p.buckets,
    accepted_steps:p.accepted_implicit_steps.length,reused_exact_probe_bases:p.accepted_implicit_steps.reduce((n,d)=>n+(d.reused_exact_probe_bases??0),0),
    accepted_endpoint_evaluations:p.accepted_implicit_steps.reduce((n,d)=>n+d.endpoint_evaluations,0),
    exact_derivative_fallbacks:p.accepted_implicit_steps.flatMap(d=>d.exact_jacobian_fallback?[d.exact_jacobian_fallback]:[]),
    sources:[capture,profile,log,`${study.output}/${c.name}.native.json`].map(source)});
}
writeFileSync(output,JSON.stringify({version:1,complete:true,pair_frames_exact,pair_transitions_exact,previous_reference_exact,cases,
  sources:[studyPath,study.plan_output,statusPath,import.meta.filename,...(previousPath?[previousPath]:[])].map(source).concat(executable),
  scope:'Sequential native profiles preserve physical frames, task transitions, recordings and contracts. Native transition clocks include task/frame overhead; cumulative stepping clocks are reported separately. Buckets nest and accepted counters exclude rejected outer attempts. This is not browser timing, timestep accuracy or hardware evidence.'},null,2)+'\n');
console.log({pair_frames_exact,pair_transitions_exact,previous_reference_exact,cases:cases.map(c=>({name:c.name,wall_s:c.unprofiled_wall_s,active_p95_s:c.native_active_transition_p95_s,reused_bases:c.reused_exact_probe_bases}))});
