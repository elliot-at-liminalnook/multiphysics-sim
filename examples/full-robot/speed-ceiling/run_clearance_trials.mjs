import fs from 'node:fs';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',catalog=JSON.parse(fs.readFileSync(`${d}/clearance-trials.json`));
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
const status=fs.existsSync(`${d}/clearance-status.json`)?JSON.parse(fs.readFileSync(`${d}/clearance-status.json`)):[];
for(const {name,prefix,planning_exit} of catalog.rows){
 if(planning_exit!==0||status.some(r=>r.name===name))continue;
 const o=fs.openSync(`${prefix}.native.json`,'wx'),e=fs.openSync(`${prefix}.native-error.txt`,'wx');
 const r=spawnSync(`${bin}/run_environment`,[`${prefix}.scene.json`,`${prefix}.config.json`,'examples/full-robot/fast-wasd/task.json',`${prefix}.actions.json`],{stdio:['ignore',o,e],timeout:90000});fs.closeSync(o);fs.closeSync(e);
 status.push({name,prefix,exit:r.status,error:r.error?.message??null});
 fs.writeFileSync(`${d}/clearance-status.json`,JSON.stringify(status,null,2)+'\n');console.log(status.at(-1));
 if(r.error)break;
}
