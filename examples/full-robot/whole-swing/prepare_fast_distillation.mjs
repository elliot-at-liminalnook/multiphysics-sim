import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',output=`${root}/fast-distillation`;
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
assert(!existsSync(`${output}/fit.json`));mkdirSync(output,{recursive:true});
const write=(name,value)=>{const path=`${output}/${name}.json`;writeFileSync(path,JSON.stringify(value)+'\n');return source(path);};
const status=read(`${root}/combined-status.json`),training=['combined-minute-1.25ms','combined-turn-1.25ms'].map(name=>{
  const c=status.cases.find(c=>c.name===name);assert(c.passed);return {capture:c.sources.find(s=>s.path.endsWith('.native.json')),acceptance:c.acceptance.source};
});
const heldout=read(`${root}/fast-distillation-holdout-status.json`).cases[0];
const capture=read(training[0].capture.path),initial=capture.frames.find(f=>f.policy).policy.observations;
const network=read('examples/full-robot/student-distillation/initial.policy.json');
for(const f of network.features)if(f.source.endsWith('.reference'))f.center=initial[f.source.replace(/\.reference$/,'.angle')];
const scene=read(`${root}/reference-minute.scene.json`),config=read(`${root}/reference-minute.config.json`);
scene.controller.sources={entry:'student.rhai',files:{'student.rhai':'fn control(t, sensors, commands, state) { for name in commands.keys() { let joint = name.sub_string(0, name.len()-7); let r = sensors[joint+".reference"]; commands[name] = r + sensors["command.tracking_gain"] * (r - sensors[joint+".angle"]); } #{commands:commands,state:state} }'}};
const descriptor={version:1,output,network:write('network-seed',network),scene:write('authored-student.scene',scene),config:write('teacher.config',config),task:source('examples/full-robot/heading-task/task.json'),
  scene_schema_omissions:source('web/leaderboard/unretained-scene-fields.json'),training,
  validation:[{capture:heldout.sources.find(s=>s.path.endsWith('.native.json')),allow_accepted_prefix:true}],
  baseline:network.outputs.map(o=>{const joint=o.target.replace(/\.target$/,'');return {target:o.target,reference:`${joint}.reference`,position:`${joint}.angle`,gain:'command.tracking_gain'};}),
  optimizer:{epochs:1000,learning_rate:.0005,batch_size:64},
  preparation_sources:[`${root}/FAST-DISTILLATION-PLAN.md`,`${root}/combined-status.json`,`${root}/fast-distillation-holdout-status.json`,`${root}/reference-minute.scene.json`,`${root}/reference-minute.config.json`,'examples/full-robot/student-distillation/initial.policy.json',import.meta.filename].map(source),
  scope:'Faster minute and mixed-steering fine teacher demonstrations for training. The predeclared mirrored held-out episode fails at 14.84 s; its accepted prefix remains validation-only, and full episode acceptance remains failed. No substitution of a passing validation case.'};
writeFileSync(`${root}/fast-distillation-study.json`,JSON.stringify(descriptor,null,2)+'\n');console.log(`${root}/fast-distillation-study.json`);
