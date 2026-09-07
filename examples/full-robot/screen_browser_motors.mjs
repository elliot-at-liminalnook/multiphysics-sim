// Reusable motor-profile screening on the versioned robot, using Rust execution.
import {readFileSync,writeFileSync,mkdirSync,openSync,closeSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory=process.argv.find(x=>x.startsWith('--directory='))?.slice(12)||'runs/full-robot/learning/browser-fidelity-screen';
const summarize=process.argv.includes('--summarize-only');
const runner='target/release/examples/run_environment';
const source='examples/full-robot/teacher-baseline/scene.json';
const config='examples/full-robot/teacher-baseline/config.json';
const task='examples/full-robot/teacher-environment.json';
const markerFile='examples/full-robot/foot-markers.json';
const read=p=>JSON.parse(readFileSync(p));
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const modes=['detailed','quasistatic_winding','quasistatic_rotor','quasistatic'];
const scene=read(source),markers=read(markerFile);
assert.equal(scene.robot.source.cad_sha256,markers.expected_cad_sha256);
mkdirSync(directory,{recursive:true});
if(!summarize){
 for(const mode of modes){
  const recipe=structuredClone(scene);recipe.options.motor_dynamics=mode;
  const input=`${directory}/${mode}.scene.json`,output=`${directory}/${mode}.native.json`;
  writeFileSync(input,JSON.stringify(recipe));const fd=openSync(output,'w');
  const result=spawnSync(runner,[input,config,task],{stdio:['ignore',fd,'inherit'],timeout:180000});closeSync(fd);
  assert.equal(result.status,0,`${mode} did not complete; inspect its capture (${result.error||''})`);
  console.log(JSON.stringify({mode,wall_s:read(output).wall_s}));
 }
}
const baseline=read(`${directory}/detailed.native.json`);
assert(baseline.completed);
const point=(frame,marker)=>{
 const p=frame.poses.find(p=>p.name===marker.link);assert(p,`missing ${marker.link}`);
 return p.position_m.map((x,i)=>x+p.rotation[i].reduce((s,r,j)=>s+r*marker.local_point_m[j],0));
};
const cases=[];
for(const mode of modes){
 const path=`${directory}/${mode}.native.json`,run=read(path);assert(run.completed);assert.equal(run.frames.length,baseline.frames.length);
 assert.equal(run.recording.scene.robot.source.cad_sha256,markers.expected_cad_sha256);
 const footMax=Object.fromEntries(markers.markers.map(m=>[m.id,0]));let angleMax=0,currentMax=0;
 for(let i=0;i<run.frames.length;i++){
  const a=run.frames[i],b=baseline.frames[i];assert.equal(a.time_s,b.time_s);
  for(const marker of markers.markers){const p=point(a,marker),q=point(b,marker);footMax[marker.id]=Math.max(footMax[marker.id],Math.hypot(...p.map((v,j)=>v-q[j])));}
  for(let j=0;j<run.contract.observations.length;j++){
   const name=run.contract.observations[j].name,d=Math.abs(run.transitions[i].observations[j]-baseline.transitions[i].observations[j]);
   if(name.endsWith('.angle'))angleMax=Math.max(angleMax,d);
   if(name.endsWith('.current'))currentMax=Math.max(currentMax,d);
  }
 }
 cases.push({mode,completed:true,simulated_s:run.transitions.at(-1).time_s,wall_s:run.wall_s,
  simulation_per_wall_second:run.transitions.at(-1).time_s/run.wall_s,
  maximum_foot_difference_m:footMax,maximum_motor_angle_difference_rad:angleMax,
  maximum_motor_current_difference_a:currentMax,capture:path,sha256:hash(path)});
}
const report={version:1,scope:'Single native diagnostic captures, including endpoint observation/render-frame preparation. No replicated timing claim, browser timing, complete task acceptance, or hardware calibration. All mechanism geometry, contact, firmware and controller remain detailed.',
 inputs:Object.fromEntries([source,config,task,markerFile,runner].map(p=>[p,hash(p)])),cases,
 decision:'Removing the fast winding/rotor storage terms alone does not establish realtime. Investigate a broader explicit browser reduction of actuator integration, internal linkage dynamics and contact geometry while retaining this detailed comparison path.'};
writeFileSync('examples/full-robot/browser-motor-screen-status.json',JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(cases.map(({mode,wall_s,simulation_per_wall_second,maximum_foot_difference_m})=>({mode,wall_s,simulation_per_wall_second,maximum_foot_difference_m})),null,2));
