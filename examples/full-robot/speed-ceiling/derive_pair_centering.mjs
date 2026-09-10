// Robot policy recipe: center each opposite pair's midstance line beneath the
// actual CAD COM. Shared Rust supplies COM/poses; this is a geometric heuristic,
// not an assertion of dynamic balance or feasible two-point static support.
import fs from 'node:fs';import crypto from 'node:crypto';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const [output,...names]=process.argv.slice(2);if(!output||!names.length)throw Error('usage: derive_pair_centering output.json source-case...');
if(fs.existsSync(output))throw Error(`refusing overwrite ${output}`);
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples',catalog=read(`${d}/clearance-trials.json`),recipes=[],evidence=[];
for(const name of names){
 const def=catalog.rows.find(r=>r.name===name);if(!def||def.planning_exit!==0)throw Error('compiled source case required');
 const plan=read(`${def.prefix}.plan.json`),supportPath=`${def.prefix}.support.json`;
 if(!fs.existsSync(supportPath)||!fs.statSync(supportPath).size){
  const r=spawnSync(`${bin}/summarize_marker_clearance`,[def.source_scene,`${def.prefix}.plan.json`,'examples/full-robot/gait-exploration/workspace-markers.json','-X-foot-surface'],{encoding:'utf8',maxBuffer:32*1024*1024});
  if(r.status!==0)throw Error(r.stderr);fs.writeFileSync(supportPath,r.stdout);
 }
 const support=read(supportPath),offsets=structuredClone(def.foot_offsets_world_m??Array.from({length:4},()=>[0,0,0])),groups=[];
 for(const [legs,time] of [[[0,2],1.0],[[1,3],.6]]){
  const f=plan.frames.find(f=>Math.abs(f.time_s-time)<1e-9),s=support.frames.find(f=>Math.abs(f.time_s-time)<1e-9).assumed_static_support;
  if(!f||!s)throw Error('exact planned midpoint sample required');
  const a=f.marker_positions_world_m[legs[0]],b=f.marker_positions_world_m[legs[1]],c=s.center_of_mass_world_m;
  const dx=b[0]-a[0],dy=b[1]-a[1],length=Math.hypot(dx,dy);if(length<.01)throw Error('degenerate support line');
  const normal=[-dy/length,dx/length],signed=normal[0]*(c[0]-(a[0]+b[0])/2)+normal[1]*(c[1]-(a[1]+b[1])/2);
  const shift=[normal[0]*signed,normal[1]*signed,0];
  for(const leg of legs)for(let j=0;j<3;j++)offsets[leg][j]+=shift[j];
  groups.push({legs,midstance_plan_time_s:time,center_of_mass_world_m:c,support_points_world_m:[a,b],normal_xy:normal,signed_com_line_distance_m:signed,added_foot_shift_world_m:shift});
 }
 const {prefix,planning_exit,planning_error,planning_sha256,scene_sha256,config_sha256,actions_sha256,...parameters}=def;
 const recipe={...parameters,name:`${name}-centered`,foot_offsets_world_m:offsets,centering_source:name};recipes.push(recipe);
 evidence.push({name,groups,source_plan_sha256:sha(`${def.prefix}.plan.json`),support_report_sha256:sha(supportPath),foot_offsets_world_m:offsets});
}
fs.writeFileSync(output,JSON.stringify(recipes,null,2)+'\n');fs.writeFileSync(output.replace(/\.json$/,'.derivation.json'),JSON.stringify({evidence,scope:'Midstance geometric centering derived from shared Rust CAD mass/pose support report. Existing displacements are retained plus a smooth planning ramp to explicit world offsets. Dynamics and collisions require independent validation.'},null,2)+'\n');
console.log(evidence.map(e=>({name:e.name,distances_mm:e.groups.map(g=>g.signed_com_line_distance_m*1000)})));
