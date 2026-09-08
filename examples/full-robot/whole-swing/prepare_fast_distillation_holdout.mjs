// A complete episode reserved before fitting the faster student.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',output='runs/full-robot/learning/fast-distillation-holdout';
const read=p=>JSON.parse(readFileSync(p)),write=(p,v)=>writeFileSync(p,JSON.stringify(v)+'\n');
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const scenePath=`${root}/reference-minute.scene.json`,configPath=`${root}/reference-minute.config.json`,task='examples/full-robot/heading-task/task.json';
const scene=read(scenePath),config=read(configPath);assert.equal(config.step_s,.00125);
const duration=32,name='teacher-heldout-mirrored-32s';
assert(!existsSync(`${output}/${name}.native.json`));mkdirSync(output,{recursive:true});
config.steps=Math.round(duration/config.step_s);
const schedule=[[0,.00375,0],[10,0,-.001],[14,.00375,0],[20,-.00125,0],[26,0,0]];
const actions=Array.from({length:Math.round(duration/.02)},(_,i)=>{
  const command=schedule.findLast(([t])=>i*.02>=t),values=scene.controller.inputs.map(c=>c.initial);
  for(const [key,value] of [['command.forward_speed',command[1]],['command.yaw_rate',command[2]]]){
    const index=scene.controller.inputs.findIndex(c=>c.name===key),c=scene.controller.inputs[index];assert(value>=c.lower&&value<=c.upper);values[index]=value;
  }
  return values;
});
const paths={};for(const [key,value] of Object.entries({scene,config,actions})){paths[key]=`${output}/${name}.${key}.json`;write(paths[key],value);}
const plan={version:1,cases:[{name,...paths,task,duration_s:duration,step_s:config.step_s,seed:0,scenario:'heldout-forward-mirrored-turn-forward-reverse-stop'}],
  reserved_schedule_s:schedule,sources:[scenePath,configPath,task,`${root}/FAST-DISTILLATION-PLAN.md`,import.meta.filename,...Object.values(paths)].map(source),
  scope:'Predeclared whole held-out teacher episode; changed command timing and yaw sign, unchanged physical recipe and existing development push. Reserved from fitting and checkpoint selection. Seed zero with explicit deterministic inputs is one case, not stochastic robustness coverage.'};
write(`${output}/plan.json`,plan);write(`${root}/fast-distillation-holdout-plan.json`,plan);console.log({output,schedule});
