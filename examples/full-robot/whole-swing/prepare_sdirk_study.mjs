import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/whole-swing',output='runs/full-robot/learning/sdirk-steering';
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const write=(path,value)=>writeFileSync(path,JSON.stringify(value)+'\n');
const teacher=read(`${root}/combined-plan.json`).cases.find(c=>c.name==='combined-turn-1.25ms');
const student=read(`${root}/fast-student-fidelity-plan.json`).cases.find(c=>c.name==='turn-20ms');
const sources=[`${root}/SDIRK2-MECHANICAL-PLAN.md`,`${root}/combined-plan.json`,`${root}/fast-student-fidelity-plan.json`,import.meta.filename,
 'crates/sim-dynamics/src/sdirk.rs','crates/sim-domain-robot/src/articulated/embedding/implicit.rs','crates/sim-domain-robot/src/articulated/embedding/mechanical_advance.rs','crates/sim-domain-robot/src/articulated/embedding/step.rs','crates/sim-runtime/src/embedded.rs'];
mkdirSync(output,{recursive:true});const cases=[];
for(const [kind,base,h,enabled] of [['student',student,.02,false],...['teacher','student'].flatMap(kind=>[.02,.01,.005].map(h=>[kind,kind==='teacher'?teacher:student,h,true]))]){
 const name=`${kind}-${enabled?'sdirk2':'be'}-${h*1000}ms`;assert(!existsSync(`${output}/${name}.native.json`));
 const config=read(base.config);config.step_s=h;config.steps=Math.round(24/h);config.report_every=Math.round(.02/h);if(enabled)config.implicit.sdirk2=true;
 const paths={};for(const [key,value] of Object.entries({scene:read(base.scene),config,actions:read(base.actions)})){paths[key]=`${output}/${name}.${key}.json`;write(paths[key],value);}
 sources.push(base.scene,base.config,base.actions,...Object.values(paths));cases.push({name,...paths,task:base.task,duration_s:24,step_s:h,seed:0,integration:enabled?'sdirk2':'backward_euler'});
}
const plan={version:1,cases,sources:[...new Set(sources)].map(source),scope:'Predeclared fixed 24-second steering inputs and CAD model. Rebuilt default-off student identity reference; teacher/student SDIRK2 at 20/10/5 ms. Each source solver tolerance is preserved. No threshold changes, model calibration or browser qualification; failed stages and task outcomes retained.'};
write(`${output}/plan.json`,plan);write(`${root}/sdirk-steering-plan.json`,plan);console.log(output);
