import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',output='runs/full-robot/learning/fast-student-fidelity';
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const write=(path,value)=>writeFileSync(path,JSON.stringify(value)+'\n');
const original=read(`${root}/fast-distillation-evaluation-plan.json`),status=read(`${root}/fast-distillation-evaluation-status.json`);assert(status.complete);
assert(status.cases.find(c=>c.name==='fitted-minute').completed,'need the complete fine minute for refinement');
const browserPath=`${root}/exact-base-turn.config.json`,browser=read(browserPath),cases=[];
const sources=[`${root}/fast-distillation-evaluation-plan.json`,`${root}/fast-distillation-evaluation-status.json`,`${root}/FAST-STUDENT-FIDELITY-PLAN.md`,browserPath,import.meta.filename];
mkdirSync(output,{recursive:true});
for(const [name,episode,step,reduced,optimized] of [
  ['turn-20ms-full','fitted-turn',.02,false,true],['turn-20ms','fitted-turn',.02,true,true],
  ['minute-20ms','fitted-minute',.02,true,true],['minute-5ms','fitted-minute',.005,true,true],
  ['minute-0.625ms','fitted-minute',.000625,false,false],
]){
  assert(!existsSync(`${output}/${name}.native.json`));
  const base=original.cases.find(c=>c.name===episode),config=read(base.config),scene=read(base.scene);
  config.step_s=step;config.steps=Math.round(base.duration_s/step);config.report_every=Math.round(.02/step);
  if(optimized){
    // Copy only the measured derivative work options, keeping every tolerance.
    assert.deepEqual(config.embedding,browser.embedding);
    const before=structuredClone(config.implicit.newton);
    config.implicit.newton.broyden_updates=true;
    for(const key of ['linearized_jacobian_probes','linearized_probe_relative_step','reuse_exact_probe_base'])config.implicit[key]=browser.implicit[key];
    const after=structuredClone(config.implicit.newton);delete after.broyden_updates;assert.deepEqual(after,before);
  }
  if(reduced){config.policy.feedback_observations=false;config.policy.task_observations.floor_forces=false;}
  const paths={};for(const [key,value] of Object.entries({scene,config,actions:read(base.actions)})){paths[key]=`${output}/${name}.${key}.json`;write(paths[key],value);}
  sources.push(base.scene,base.config,base.actions,...Object.values(paths));
  cases.push({name,...paths,task:base.task,duration_s:base.duration_s,step_s:step,seed:0,reduced_unused_feedback:reduced,optimized_derivative_work:optimized});
}
const plan={version:1,cases,sources:[...new Set(sources)].map(source),scope:'Fixed fitted network, unchanged CAD physical definition and tolerances. Fine minute refinement and explicit browser derivative-work profile. Full/reduced 20 ms steering tests omission identity separately. All original physical, trajectory and browser thresholds remain fixed; all failures retained.'};
write(`${output}/plan.json`,plan);write(`${root}/fast-student-fidelity-plan.json`,plan);console.log(output);
