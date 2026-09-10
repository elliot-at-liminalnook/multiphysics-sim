import fs from 'node:fs';const d='examples/full-robot/gait-exploration',read=p=>JSON.parse(fs.readFileSync(`${d}/${p}`));
let plan=read('opposite-pair-clear-flight.plan.json'),config=read('opposite-pair-clear-flight.config.json'),scene=read('learning-teacher.scene.json'),task=read('student.task.json');
let samples=plan.trajectory.keyframes.filter(k=>k.time_s>=.4-1e-9&&k.time_s<=1.2+1e-9).map(k=>k.values);
config.motors.target_trajectory=null;config.policy.feedback_observations=false;config.policy.neural_residual=null;config.world_loads=null;
let indices=Object.fromEntries(config.motors.target_coordinates.map((n,i)=>[n.replace(/^joint\./,'')+'.target',i]));
scene.controller.sources={entry:'paired-phase.rhai',files:{'paired-phase.rhai':`
fn control(t, sensors, commands, state) {
 let p = parameters();
 if !state.contains("phase") { state.phase = 0.0; state.rate = 0.0; state.last_t = t; state.yaw_offsets = [0.0,0.0,0.0,0.0]; }
 let dt = t - state.last_t; state.last_t = t;
 state.phase = (state.phase + dt * state.rate) % p.period_s;
 if state.phase < 0.0 { state.phase += p.period_s; }
 let u = state.phase % 0.4;
 let all_stance = u <= 0.08 || u >= 0.32;
 let requested = sensors["command.forward_speed"] / p.nominal_speed_m_s;
 if requested != 0.0 && requested * state.rate >= 0.0 { state.rate = requested; }
 else if all_stance { state.rate = requested; }
 let yaw = if requested != 0.0 { sensors["command.yaw_rate"] } else { 0.0 };
 for leg in 0..4 {
  let first_pair = leg == 0 || leg == 2;
  let active_pair = (state.phase < 0.4 && first_pair) || (state.phase >= 0.4 && !first_pair);
  if active_pair && u > 0.08 && u < 0.32 {
   state.yaw_offsets[leg] *= 0.65;
  } else {
   state.yaw_offsets[leg] = (state.yaw_offsets[leg] - p.yaw_jacobian_ratios[leg] * yaw * dt).max(-0.05).min(0.05);
  }
 }
 let cell = state.phase / 0.02; let i = cell.floor().to_int(); let blend = cell - i.to_float();
 for name in commands.keys() {
  let j = p.motor_indices[name]; let target = p.samples[i][j] + blend * (p.samples[i+1][j] - p.samples[i][j]);
  if j % 3 == 0 { target += state.yaw_offsets[j / 3]; }
  let joint = name.sub_string(0,name.len()-7);
  commands[name] = target + sensors["command.tracking_gain"] * (target - sensors[joint + ".angle"]);
 }
 #{commands:commands,state:state}
}`}};
let ws=read('workspace-results.json').rows.find(x=>x.id==='coupled-same-hip0-foot-60');
let b=plan.frames[0].poses.find(x=>x.name.includes('Chassis')).position_m;
let ratios=ws.markers.map((m,i)=>Math.hypot(m.position_world_m[0]-b[0],m.position_world_m[1]-b[1])/Math.hypot(m.jacobian[0][3*i],m.jacobian[1][3*i]));
scene.controller.parameters={period_s:.8,nominal_speed_m_s:.025,samples,motor_indices:indices,yaw_jacobian_ratios:ratios};
scene.controller.inputs.forEach(c=>{if(c.name==='command.forward_speed'){c.lower=-.025;c.upper=.025;c.initial=0}if(c.name==='command.yaw_rate'){c.lower=-.03;c.upper=.03;c.initial=0}});
let actions=read('learning.actions.json');
for(let [suffix,step] of [['',.00125],['-5ms',.005]]){let c=structuredClone(config);c.step_s=step;c.steps=Math.round(6/step);c.report_every=Math.round(.02/step);fs.writeFileSync(`${d}/live-pair${suffix}.config.json`,JSON.stringify(c)+'\n')}
fs.writeFileSync(`${d}/live-pair.scene.json`,JSON.stringify(scene)+'\n');fs.writeFileSync(`${d}/live-pair.task.json`,JSON.stringify(task)+'\n');
fs.writeFileSync(`${d}/live-pair-design.json`,JSON.stringify({version:1,source_plan:'opposite-pair-clear-flight.plan.json',cycle_s:.8,commanded_speed_range_m_s:[-.025,.025],yaw_rate_range_rad_s:[-.03,.03],yaw_jacobian_ratios:ratios,scope:'Rhai phase controller through shared Rust actuator/physics runtime. Commands set signed phase rate; stopping and reversing wait for an all-stance phase. Stance hip offsets integrate yaw using initial CAD Jacobian ratios and relax during swing. Walking turns only; no lateral motion or turn-in-place claim. No ideal world-position/foot/contact feedback consumed. Experimental, not stability certified.'},null,2)+'\n');
