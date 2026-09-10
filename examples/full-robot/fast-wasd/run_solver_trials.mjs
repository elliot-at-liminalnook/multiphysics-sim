import fs from 'node:fs';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/fast-wasd',read=p=>JSON.parse(fs.readFileSync(`${d}/${p}.json`)),source='braked-5ms';
const cases=[{name:'braked-cache3',options:{cached_mechanical_iteration_limit:3}},{name:'braked-fresh-sample',options:{reuse_controller_sample_jacobian:false}}],status=[],profile=read('profile-plan'),human=read('human-trials');
for(const {name,options} of cases){
 const c=read(`${source}.config`);Object.assign(c.implicit,options);
 for(const suffix of ['scene','actions'])fs.copyFileSync(`${d}/${source}.${suffix}.json`,`${d}/${name}.${suffix}.json`);fs.writeFileSync(`${d}/${name}.config.json`,JSON.stringify(c)+'\n');
 const def={name,step_s:c.step_s,solver_options:options};profile.cases.push(def);human.cases.push({...def,source,speed_m_s:.065});
 fs.writeFileSync(`${d}/profile-plan.json`,JSON.stringify(profile,null,2)+'\n');fs.writeFileSync(`${d}/human-trials.json`,JSON.stringify(human,null,2)+'\n');
 const o=fs.openSync(`${d}/${name}.native.json`,'wx'),e=fs.openSync(`${d}/${name}.error.txt`,'wx');
 const r=spawnSync('/Users/elliot/physics-simulator/target/gait-exploration/release/examples/run_environment',[`${d}/${name}.scene.json`,`${d}/${name}.config.json`,`${d}/task.json`,`${d}/${name}.actions.json`],{stdio:['ignore',o,e],timeout:120000});fs.closeSync(o);fs.closeSync(e);
 status.push({name,exit:r.status,error:r.error?.message});fs.writeFileSync(`${d}/solver-status.json`,JSON.stringify(status,null,2)+'\n');console.log(status.at(-1));if(r.error)break;
}
