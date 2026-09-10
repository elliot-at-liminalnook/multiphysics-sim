import fs from 'node:fs';
const dir='examples/full-robot/gait-exploration';
const original=JSON.parse(fs.readFileSync('examples/full-robot/whole-swing/settled-integral-minute.config.json'));
const ws=JSON.parse(fs.readFileSync(`${dir}/workspace-results.json`));
const pose=ws.rows.find(x=>x.id==='coupled-same-hip0-foot-60');
if(pose.error||pose.sampled_penetrations.length)throw Error('initial crouch failed sampled geometry');
const scene=JSON.parse(fs.readFileSync('examples/full-robot/whole-swing/settled-integral-candidate.scene.json'));
const dz=scene.robot.world.floor_z+0.0004-pose.markers.reduce((a,m)=>a+m.position_world_m[2],0)/4;
const order=[0,2,1,3];
const definitions=[
 {name:'long-stride-wave',family:'sequential swing; body advances in four-contact transfer',stride_m:0.08,transfer_s:1.6,swing_s:0.64,lift_m:0.015,continuous:false,groups:order.map(i=>[i]),cycles:2},
 {name:'continuous-wave',family:'sequential swing with continuous body advance',stride_m:0.08,transfer_s:0.8,swing_s:0.48,lift_m:0.015,continuous:true,groups:order.map(i=>[i]),cycles:2},
 {name:'opposite-pair',family:'alternating opposite pairs; dynamic line support',stride_m:0.02,transfer_s:0.4,swing_s:0.24,lift_m:0.01,continuous:true,groups:[[0,2],[1,3]],cycles:6},
];
const smooth=u=>u*u*u*(10+u*(-15+6*u));
const bounds=original.motors.effective.components.map((m,i)=>({lower:i%3===0?-Math.PI/3:i%3===1?original.initial_coordinates[i]-Math.PI*5/6:-2.5,upper:i%3===0?Math.PI/3:i%3===1?original.initial_coordinates[i]+Math.PI*5/6:-0.02,max_step:i%3===0?0.05:0.1}));
for(let def of definitions){
 let transfers=def.cycles*def.groups.length;
 let hold=0.4,duration=hold+transfers*def.transfer_s+0.8;
 let body=[],feet=[];
 for(let tick=0;tick<=Math.round(duration/0.02);tick++){
  let t=Number((tick*0.02).toFixed(8)),elapsed=Math.max(0,Math.min(t-hold,transfers*def.transfer_s));
  let k=Math.min(transfers-1,Math.floor(elapsed/def.transfer_s));
  let u=elapsed/def.transfer_s-k; if(elapsed===transfers*def.transfer_s)u=1;
  let swing0=(1-def.swing_s/def.transfer_s)/2,swing1=1-swing0;
  let su=Math.max(0,Math.min(1,(u-swing0)/(swing1-swing0)));
  let group=def.groups[k%def.groups.length];
  let advancePerTransfer=def.stride_m/def.groups.length;
  let advance=advancePerTransfer*(k+(def.continuous?u:smooth(Math.min(1,u/swing0))));
  let offset=[0,0,0];
  // Three-foot support body shifts follow the existing robot's direction, with
  // zero shifts during initial/final holds; no static guarantee is assumed.
  if(def.groups.length===4){let i=group[0];let offsets=original.policy.step_reference.sequence.support_offsets_m;
   let envelope=u<swing0?smooth(u/swing0):u>swing1?smooth((1-u)/(1-swing1)):1;
   offset=offsets[i].map(x=>x*envelope);
  }
  let f=Array(12).fill(0);
  for(let i=0;i<4;i++){
   let phaseIndex=def.groups.findIndex(g=>g.includes(i));
   let completed=Math.floor(k/def.groups.length)+(phaseIndex<k%def.groups.length?1:0);
   f[3*i]=def.stride_m*(completed+(group.includes(i)?smooth(su):0));
   if(group.includes(i))f[3*i+2]=def.lift_m*16*su*su*(1-su)*(1-su);
  }
  if(t<hold){advance=0;offset=[0,0,0];f.fill(0)}
  if(t>=hold+transfers*def.transfer_s){advance=def.cycles*def.stride_m;offset=[0,0,0];for(let i=0;i<4;i++){f[3*i]=advance;f[3*i+2]=0}}
  body.push({time_s:t,values:[advance+offset[0],offset[1],offset[2]]});feet.push({time_s:t,values:f});
 }
 const config={expected_cad_sha256:original.policy.task_observations.expected_cad_sha256,independent_coordinates:original.motors.effective.components.map(m=>m.dof),initial_coordinates:pose.coordinates,initial_base_translation_m:[0,0,dz],bounds,embedding:original.embedding,placement:original.policy.step_reference.placement,sample_period_s:0.02,maximum_interpolation_error_m:0.0001,displacements_world_m:{interpolation:'linear',keyframes:feet},base_displacements_world_m:{interpolation:'linear',keyframes:body}};
 fs.writeFileSync(`${dir}/${def.name}.planning.json`,JSON.stringify(config)+'\n');
 Object.assign(def,{duration_s:duration,predicted_schedule_speed_mm_s:1000*def.stride_m/(def.groups.length*def.transfer_s),initial_coordinates:pose.coordinates,initial_base_translation_m:[0,0,dz],planning:`${dir}/${def.name}.planning.json`,acceptance:{screen_only:true,minimum_actual_speed_mm_s:12.5,maximum_heading_error_rad:0.02,maximum_tilt_rad:0.1,maximum_sampled_inter_link_penetration_m:0,maximum_loaded_material_motion_to_body_advance_ratio:0.05,maximum_stop_drift_m:0.003}});
}
fs.writeFileSync(`${dir}/candidates.json`,JSON.stringify({version:1,definitions,notes:['Prospective development recipes. Predicted schedule speeds are not measured capabilities.','Crouched foot-crank posture retains extension reserve for long fore-aft strokes. CAD robot unchanged.','Fixed offline references test coordination hypotheses; no learned control, live steering or disturbance adaptation claim.','Support/centroidal feasibility still needs evaluation. Paired line support cannot be certified with a three-point static polygon.']},null,2)+'\n');
console.log(definitions.map(d=>({name:d.name,speed_mm_s:d.predicted_schedule_speed_mm_s,duration_s:d.duration_s,base:d.initial_base_translation_m})));
