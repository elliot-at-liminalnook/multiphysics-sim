// Robot-specific steering recipe from the shared Rust CAD Jacobian inspector.
import fs from 'node:fs';import {spawnSync} from 'node:child_process';
export function cadYawRatios(scenePath,config,prefix){
 const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
 const recipe={record_poses:true,independent_coordinates:config.motors.target_coordinates,embedding:config.embedding,
  samples:[{id:'initial-steering',coordinates:config.initial_coordinates}]};
 const input=`${prefix}.yaw-inspection-recipe.json`,output=`${prefix}.yaw-inspection.json`;
 if(fs.existsSync(input)||fs.existsSync(output))throw Error('refusing overwrite yaw inspection');
 fs.writeFileSync(input,JSON.stringify(recipe,null,2)+'\n');
 const r=spawnSync(`${bin}/inspect_configurations`,[scenePath,'examples/full-robot/gait-exploration/workspace-markers.json',input],{encoding:'utf8',maxBuffer:32*1024*1024});
 if(r.status!==0)throw Error(r.stderr);fs.writeFileSync(output,r.stdout);const row=JSON.parse(r.stdout).rows[0];
 if(row.error||row.authored_limit_violations.length||row.sampled_penetrations.length)throw Error('valid initial CAD pose required for steering');
 const bodies=row.poses.filter(p=>p.name.includes('Chassis'));if(bodies.length!==1)throw Error('unique chassis COM pose required');
 const body=bodies[0].position_m;
 const legs=row.markers.map((m,leg)=>{
  const j=[m.jacobian[0][3*leg],m.jacobian[1][3*leg]],r=[m.position_world_m[0]-body[0],m.position_world_m[1]-body[1]];
  const desired=[-r[1],r[0]],norm=j[0]*j[0]+j[1]*j[1];if(norm<1e-12)throw Error('degenerate hip yaw Jacobian');
  const ratio=(j[0]*desired[0]+j[1]*desired[1])/norm;
  return {marker:m.id,ratio,unachieved_xy_fraction:Math.hypot(desired[0]-ratio*j[0],desired[1]-ratio*j[1])/Math.hypot(...desired)};
 });
 fs.writeFileSync(`${prefix}.yaw-derivation.json`,JSON.stringify({legs,scope:'Initial CAD Jacobian least-squares projection of world yaw foot velocity onto each existing hip-only steering correction. Residual is reported; this is not exact whole-body yaw control. Controller negates these ratios to hold stance feet during body yaw.'},null,2)+'\n');
 return legs.map(l=>l.ratio);
}

export function cadYawCoordinates(scenePath,config,prefix){
 cadYawRatios(scenePath,config,prefix);
 const read=p=>JSON.parse(fs.readFileSync(p)),row=read(`${prefix}.yaw-inspection.json`).rows[0];
 const body=row.poses.find(p=>p.name.includes('Chassis')).position_m,scene=read(scenePath),p=scene.controller.parameters;
 const directions=row.markers.map(m=>{const r=[m.position_world_m[0]-body[0],m.position_world_m[1]-body[1]],radius=Math.hypot(...r);if(radius<.001)throw Error('degenerate yaw radius');return {direction:[-r[1]/radius,r[0]/radius,0],radius};});
 const recipe=read('examples/full-robot/speed-ceiling/capability-recipe.json');
 recipe.inspection=read(`${prefix}.yaw-inspection-recipe.json`);recipe.directions_world=directions.map(d=>d.direction);recipe.support_groups=[];
 recipe.reference_cycle={period_s:p.period_s,stride_m:p.period_s*p.nominal_speed_m_s,samples:p.samples};
 recipe.actuators=Object.fromEntries(config.motors.effective.components.map(m=>[m.dof,m.parameters]));
 recipe.provenance={scope:'Initial stance-foot world yaw velocity mapped through all three CAD joint coordinates per leg. This linearization is used only for bounded controller target offsets.'};
 const input=`${prefix}.yaw-full-recipe.json`,output=`${prefix}.yaw-full-capability.json`;
 fs.writeFileSync(input,JSON.stringify(recipe,null,2)+'\n');
 const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
 const r=spawnSync(`${bin}/analyze_motion_capability`,[scenePath,'examples/full-robot/gait-exploration/workspace-markers.json',input],{encoding:'utf8',maxBuffer:32*1024*1024});
 if(r.status!==0)throw Error(r.stderr);fs.writeFileSync(output,r.stdout);
 const report=JSON.parse(r.stdout),coefficients=Array(config.motors.target_coordinates.length).fill(0),legs=[];
 for(const [leg,m] of row.markers.entries()){
  const f=report.rows[0].directions[leg].feet.find(f=>f.marker===m.id),{radius,direction}=directions[leg];
  const values=f.bound.coordinate_rates_per_unit.map(v=>v*radius);
  f.coordinate_names.forEach((name,j)=>{const i=config.motors.target_coordinates.indexOf(name);if(i<0)throw Error('missing coordinate');coefficients[i]=values[j];});
  const desired=direction.map(v=>v*radius),achieved=m.jacobian.map(row=>row.reduce((s,v,i)=>s+v*coefficients[i],0));
  const residual=Math.hypot(...desired.map((v,i)=>v-achieved[i]));if(residual>1e-10)throw Error('yaw Jacobian reproduction failed');
  legs.push({marker:m.id,coordinate_names:f.coordinate_names,coefficients:values,maximum_yaw_rate_from_no_load_budget_rad_s:f.bound.maximum_task_speed/radius,velocity_residual_m_s:residual});
 }
 fs.writeFileSync(`${prefix}.yaw-full-derivation.json`,JSON.stringify({legs,coefficients,scope:'Exact initial CAD Jacobian mapping of world yaw foot velocity through all three independent joints. Controller subtracts these coefficients times requested yaw rate while in stance. Bounded offsets reset during swing. Not exact finite-angle or time-varying whole-body yaw control.'},null,2)+'\n');
 return coefficients;
}
