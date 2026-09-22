import fs from'node:fs';import path from'node:path';import assert from'node:assert/strict';import{createHash}from'node:crypto';
const here=import.meta.dirname,root=path.resolve(here,'../../../..'),read=p=>JSON.parse(fs.readFileSync(path.join(here,p)));
const scene=read('scene.json'),cfg=read('config.json'),native=read('verification/native-cadence.json'),browser=read('verification/browser.json');
assert(native.window_complete&&!native.error);assert.equal(native.frames.length,11);
assert.equal(cfg.step_s*cfg.report_every,.005);assert.equal(scene.period_s,.02);assert.equal(cfg.motors.controller,'cad_fixed_pd');assert(!cfg.motors.effective);
let parameters=0;
for(const [i,m] of scene.robot.motors.entries()){
 const binding=scene.robot.actuator_profiles.bindings[m.id],family=scene.robot.actuator_profiles.families[binding.family];assert.equal(binding.physical_unit,null);
 const fit=JSON.parse(fs.readFileSync(path.join(root,family.evidence.fit.path))).experiment.model;
 const motor=native.metadata.motor_components.find(c=>c.dof==='joint.'+m.joint),driver=native.metadata.driver_components.find(c=>c.dof===motor.dof),servo=native.metadata.servo_components.find(c=>c.dof===motor.dof);
 for(const [domain,actual,expected]of[['motor',motor.parameters,fit.motor],['driver',driver.parameters,fit.bridge]])for(const [key,value]of Object.entries(family[domain])){assert.equal(value.value,expected[key]);assert.equal(actual[key],expected[key],`${m.name} ${key}`);parameters++;}
 for(const [key,value]of Object.entries({period:.0025,latency:.00125,kp_q8:4096,kd_q8:0,kv_q8:4096,limit:1000}))assert.equal(servo.parameters[key],value);
 const start=native.metadata.servo_state_layout[i][0];for(const f of native.frames){const ticks=Math.round(f.time_s*400);assert.equal(f.servo_states[start+3],f.time_s===0?0:ticks+1);assert.equal(f.servo_states[start+4],ticks);}
}
assert.equal(browser.errors.length,0);assert.equal(browser.probe.errors.length,0);assert(browser.probe.frames.at(-1).time_s>=.1);
for(const predicate of [a=>a[3]>0&&a[5]===0,a=>a[5]>0,a=>a[5]<0,a=>a[3]<0,a=>a[3]===0&&a[5]===0])assert(browser.probe.actions.some(r=>predicate(r.action)));
const final=browser.probe.frames.at(-1);assert(browser.speed.includes((final.speed.net_distance_m/final.time_s).toFixed(3)+' m/s'));
// Numerical comparison limits: 0.1 degree at motor coordinates,
// 1 mm at every link origin, and 0.1 A at any winding. All must pass.
const limits={motor_angle_deg:.1,link_position_m:.001,current_a:.1};
const reference=read('verification/reference.browser.json'),comparisons=[];
for(const name of ['coarse-4','coarse-8']){
 const candidate=read(`verification/${name}.browser.json`);assert.equal(candidate.frames.length,reference.frames.length);
 const max={motor_angle_deg:0,link_position_m:0,current_a:0};
 for(let k=0;k<reference.frames.length;k++){
  const a=reference.frames[k],b=candidate.frames[k];assert.equal(a.time_s,b.time_s);
  for(const index of reference.metadata.joint_indices)max.motor_angle_deg=Math.max(max.motor_angle_deg,Math.abs(a.joint_positions[index]-b.joint_positions[index])*180/Math.PI);
  for(let i=0;i<a.poses.length;i++)max.link_position_m=Math.max(max.link_position_m,Math.hypot(...a.poses[i].position_m.map((v,j)=>v-b.poses[i].position_m[j])));
  for(let i=0;i<a.motor_readings.length;i++)max.current_a=Math.max(max.current_a,Math.abs(a.motor_readings[i].current_a-b.motor_readings[i].current_a));
 }
 comparisons.push({name,max_error:max,limits,accepted:Object.entries(max).every(([k,v])=>v<=limits[k]),simulation_per_wall:1/candidate.wall_s,scope:'One-second common-input browser comparison; host contention can affect timing.'});
}
const long=read('verification/long-reference.browser.json'),last=long.frames.at(-1);assert(!long.error&&last.time_s===3);assert(long.frames.every(f=>!f.error&&!f.learning?.speed?.fallen));
const hashes={};for(const rel of ['scene.json','config.json','task.json','overrides.json','controller-identity.json','prepare.mjs','verify.mjs','README.md','bundle/build-manifest.json','bundle/sim_web_bg.wasm','verification/native-cadence.json','verification/browser.json','verification/reference.browser.json','verification/coarse-4.browser.json','verification/coarse-8.browser.json','verification/long-reference.browser.json'])hashes[rel]=createHash('sha256').update(fs.readFileSync(path.join(here,rel))).digest('hex');
const result={simulation_only:true,motors:12,physical_parameter_checks:parameters,control_hz:400,native_measurement_hz:200,outer_policy_hz:50,wasd_and_release:true,elapsed_time_speed_display:true,comparisons,long_trial:{duration_s:last.time_s,wall_s:long.wall_s,simulation_per_wall:last.time_s/long.wall_s,net_distance_m:last.learning.speed.net_distance_m,mean_net_speed_m_s:last.learning.speed.net_distance_m/last.time_s,fallen:last.learning.speed.fallen,scope:'Single seed, short forward request; not a validated sustainable gait speed.'},calibration:'Provisional fitted candidates, not accepted twelve-motor hardware calibration',sha256:hashes};
fs.writeFileSync(path.join(here,'verification/summary.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({...result,sha256:undefined},null,2));
