// Optional shared planner behavior; robot physics and learned weights unchanged.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root='examples/full-robot/prelift-control';
const run=process.argv[2]??'runs/full-robot/learning/prelift-control';
const scenePath='examples/full-robot/student-distillation/scene.json';
const configPath='examples/full-robot/browser-precision/guarded-short.config.json';
const taskPath='examples/full-robot/heading-task/task.json';
const read=p=>JSON.parse(readFileSync(p)),write=(p,v)=>writeFileSync(p,JSON.stringify(v)+'\n');
const scene=read(scenePath),base=read(configPath),task=read(taskPath);
const v=base.policy.step_reference.sequence.maximum_speed_m_s;
const w=base.policy.step_reference.sequence.maximum_yaw_rate_rad_s;
const cases=[
 {name:'default-turn',enabled:false,schedule:[[0,v,0],[8.4,0,w],[16.8,-v,0],[20,0,0]]},
 {name:'enabled-turn',schedule:[[0,v,0],[8.4,0,w],[16.8,-v,0],[20,0,0]]},
 {name:'cancel-resume',schedule:[[0,v,0],[.4,0,0],[2,v,0],[16.8,0,0]]},
 {name:'cancel-later-resume',schedule:[[0,v,0],[4.8,0,0],[6,v,0],[16.8,0,0]]},
 {name:'prelift-reverse',schedule:[[0,v,0],[.4,-v,0],[8.4,v,0],[16.8,0,0]]},
 {name:'default-early-reverse',enabled:false,schedule:[[0,v,0],[.4,-v,0],[8.4,v,0],[16.8,0,0]]},
 {name:'prelift-turn',schedule:[[0,v,0],[.4,0,w],[8.4,v,0],[16.8,0,0]]},
 {name:'default-cancel-resume',enabled:false,schedule:[[0,v,0],[.4,0,0],[2,v,0],[16.8,0,0]]},
 {name:'cancel-resume-refined',step_s:.005,schedule:[[0,v,0],[.4,0,0],[2,v,0],[16.8,0,0]]},
 {name:'prelift-turn-refined',step_s:.005,schedule:[[0,v,0],[.4,0,w],[8.4,v,0],[16.8,0,0]]},
];
mkdirSync(run,{recursive:true});
for(const c of cases){
 const config=structuredClone(base);
 if(c.enabled!==false)config.policy.step_reference.sequence.update_command_before_lift=true;
 if(c.step_s){const duration=config.steps*config.step_s;config.step_s=c.step_s;config.steps=Math.round(duration/config.step_s);config.report_every=Math.round(task.period_s/config.step_s);}
 const actions=Array.from({length:Math.round(config.steps*config.step_s/task.period_s)},(_,i)=>{
  const time=i*task.period_s,s=c.schedule.findLast(s=>s[0]<=time+1e-12);
  return scene.controller.inputs.map(input=>input.name==='command.forward_speed'?s[1]:input.name==='command.yaw_rate'?s[2]:input.initial);
 });
 c.config=`${run}/${c.name}.config.json`;c.actions=`${run}/${c.name}.actions.json`;
 write(c.config,config);write(c.actions,actions);
}
const browser=structuredClone(base);browser.policy.step_reference.sequence.update_command_before_lift=true;
write(`${root}/browser.config.json`,browser);
const plan={version:1,scene:scenePath,task:taskPath,cases,
 sources:[scenePath,configPath,taskPath,`${root}/prepare.mjs`].map(path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')})),
 scope:'Optional non-reversing command reconsideration at the pre-lift support boundary. Translation reversals retain the committed transfer; same CAD physics, actuator limits, network weights and numerical/physical acceptance. Default/enabled turn and early reversal recipes test retained behavior. Canceled transfers must not earn walking credit.',
 prototype:{source_commit:'f3ea689',capture:`${run}/prototype-early-reverse.native.json`,note:'The first opt-in implementation also retargeted translation reversals before lift-off; it failed a later foot-motor command bound. Retain this rejected prototype separately from the guarded implementation.'}};
write(`${run}/plan.json`,plan);write(`${root}/plan.json`,plan);console.log(run);
