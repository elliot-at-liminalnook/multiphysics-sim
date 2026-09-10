import fs from 'node:fs';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/fast-wasd',read=p=>JSON.parse(fs.readFileSync(`${d}/${p}.json`)),source='human-braked-65',cases=[
 {name:'braked-5ms',step_s:.005},
 {name:'braked-5ms-loose',step_s:.005,tolerance:1e-4},
 {name:'braked-10ms-loose',step_s:.01,tolerance:1e-4},
 {name:'braked-fine',step_s:.000625},
],status=[],human=read('human-trials');
fs.writeFileSync(`${d}/profile-plan.json`,JSON.stringify({source,cases,acceptance:{maximum_matched_body_difference_m:.003,maximum_directed_speed_difference_fraction:.05,maximum_loaded_material_motion_ratio:.05,maximum_tilt_rad:.1,maximum_post_transfer_stop_drift_m:.003,browser_simulation_per_wall:1,browser_p95_transition_s:.02},scope:'Explicit integration approximations. Keep CAD geometry, effective actuators and floor law unchanged. Validate recorded physical differences before selecting a browser profile.'},null,2)+'\n');
for(const definition of cases){
 const {name,step_s,tolerance}=definition,scene=read(`${source}.scene`),c=read(`${source}.config`);
 c.step_s=step_s;c.steps=Math.round(20/step_s);c.report_every=Math.round(.02/step_s);
 if(tolerance){c.implicit.newton.absolute_tolerance=tolerance;c.implicit.newton.relative_tolerance=tolerance;}
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',read(`${source}.actions`)]])fs.writeFileSync(`${d}/${name}.${suffix}.json`,JSON.stringify(value)+'\n');
 human.cases.push({...definition,source,speed_m_s:.065});fs.writeFileSync(`${d}/human-trials.json`,JSON.stringify(human,null,2)+'\n');
 const o=fs.openSync(`${d}/${name}.native.json`,'wx'),e=fs.openSync(`${d}/${name}.error.txt`,'wx');
 const r=spawnSync('/Users/elliot/physics-simulator/target/gait-exploration/release/examples/run_environment',[`${d}/${name}.scene.json`,`${d}/${name}.config.json`,`${d}/task.json`,`${d}/${name}.actions.json`],{stdio:['ignore',o,e],timeout:150000});fs.closeSync(o);fs.closeSync(e);
 status.push({name,exit:r.status,error:r.error?.message});fs.writeFileSync(`${d}/profile-status.json`,JSON.stringify(status,null,2)+'\n');console.log(status.at(-1));if(r.error)break;
}
