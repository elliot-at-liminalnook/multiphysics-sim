// A short command-association check, not a sustained performance certificate.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const output=process.argv[2]??'runs/interactive/command-response';
const scenePath='examples/full-robot/student-distillation/scene.json';
const configPath='examples/full-robot/browser-precision/guarded-short.config.json';
const taskPath='examples/full-robot/heading-task/task.json';
const read=p=>JSON.parse(readFileSync(p));
const scene=read(scenePath),config=read(configPath),task=read(taskPath);
config.steps=Math.round(10/config.step_s);
const actions=Array.from({length:Math.round(10/task.period_s)},(_,i)=>scene.controller.inputs.map(c=>
 c.name==='command.forward_speed'&&i*task.period_s<6?c.upper:c.initial));
mkdirSync(output,{recursive:true});
for(const [kind,value] of Object.entries({config,actions}))writeFileSync(`${output}/probe.${kind}.json`,JSON.stringify(value)+'\n');
const plan={version:1,duration_s:10,forward_until_s:6,preset:'robot-browser-solver',scene:scenePath,task:taskPath,
 sources:[scenePath,configPath,taskPath,'examples/full-robot/browser-response/prepare.mjs'].map(path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')})),
 scope:'Inherited physical/controller profile and complete seven-second load schedule. Six-second forward request followed by release. Used to verify association of command, received reference and actual drawn snapshot; no sustained walking or hardware-response certificate.'};
writeFileSync(`${output}/probe.plan.json`,JSON.stringify(plan,null,2)+'\n');
console.log(output);
