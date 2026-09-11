// Prepare a driving experiment using the versioned CAD and existing controller.
// All dynamics, actuator execution, observations and scoring stay in Rust.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const [output, profile='rates', maneuver='forward'] = process.argv.slice(2);
assert(output && ['rates','original'].includes(profile) && ['forward','turn'].includes(maneuver), 'usage: prepare_drive_benchmark.mjs fresh-input.json [rates|original] [forward|turn]');
const root = path.dirname(import.meta.filename);
const read = name => JSON.parse(fs.readFileSync(path.join(root, name)));
const robot = read('baseline/robot.simrobot.json');
const period = .02, duration = 10, step = .000125;
const config = read('imu-policy.config.json');
config.step_s = step; config.steps = duration / step; config.report_every = period / step;
// Existing shared formulation avoids subtracting nearby endpoint states when
// computing auxiliary rates on tiny hybrid-event intervals. Tolerances and
// physical component residuals are unchanged.
if(profile==='rates')config.implicit.auxiliary_rate_unknowns=true;
delete config.motors.target_trajectory;
// Continuous CAD axles; software reference bounds cover this finite experiment.
config.policy.target_bounds_rad = {'joint.left axle':[-200,200], 'joint.right axle':[-200,200]};
const scene = {version:1, robot, options:{contact:true,flex:false}, period_s:period, duration_s:duration,
 controller:{sources:{entry:'velocity-controller.rhai',files:{'velocity-controller.rhai':fs.readFileSync(path.join(root,'velocity-controller.rhai'),'utf8')}},
 parameters:{period_s:period, initial_left:config.motors.servos[0].target_rad,initial_right:config.motors.servos[1].target_rad},
 inputs:['left','right'].map(name=>({name:`command.${name}_speed`,kind:'AngularVelocity',lower:-10,upper:10,initial:0}))}};
const observations=[];
for(const axis of ['x','y','z']) {
 for(const [label,kind] of [['position','body_position'],['velocity','body_velocity'],['angular_velocity','body_angular_velocity']])
  observations.push({name:`body.${label}.${axis}`,source:{kind,link:'chassis',axis}});
 observations.push({name:`body.forward.${axis}`,source:{kind:'body_axis',link:'chassis',body_axis:'x',world_axis:axis}});
}
for(const joint of robot.joints.filter(j=>j.type==='continuous'))for(const [label,kind] of [['angle','coordinate_position'],['speed','coordinate_velocity']])
 observations.push({name:`${joint.name}.${label}`,source:{kind,coordinate:`joint.${joint.name}`}});
for(const name of ['command.left_speed','command.right_speed'])observations.push({name,source:{kind:'controller_input',name}});
const task={version:1,observation_source:'ideal_runtime_teacher_only',period_s:period,observations,rewards:[],termination_bounds:[],speed:{body_link:'chassis'}};
// Commands follow CAD joint coordinates, not motor housing shaft directions.
// This CAD declares both axle joint axes as chassis +Y, so forward uses equal signs.
for(const name of ['left axle','right axle'])assert.deepEqual(robot.joints.find(j=>j.name===name).axis,[0,1,0]);
const stages=[{start_s:0,end_s:1,values:[0,0]},{start_s:1,end_s:8,values:maneuver==='forward'?[5,5]:[5,-5]},{start_s:8,end_s:10,values:[0,0]}];
const input_events=stages.map(s=>({at_step:Math.round(s.start_s/step),values:s.values}));
const record={version:1,kind:'sampled_environment_recording',error:null,task,
 runtime:{version:3,kind:'embedded_session',scene,config,seed:31,completed_steps:config.steps,input_events}};
const bytes=JSON.stringify(record)+'\n'; fs.writeFileSync(output,bytes,{flag:'wx'});
fs.writeFileSync(output+'.spec.json',JSON.stringify({version:1,profile,maneuver,input_sha256:createHash('sha256').update(bytes).digest('hex'),cad_sha256:robot.source.cad_sha256,stages,
 scope:'Planned 10 s contact-enabled driving experiment. Original CAD unchanged. Existing reference-integrating Rhai controller commands ordinary CAD servos. Command/reference bounds are software experiment choices, not identified hardware limits. completed_steps denotes planned coverage, not measured completion.'},null,2)+'\n',{flag:'wx'});
