// Configuration authoring only; shared Rust generates and executes every step.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const out=process.argv[2]||'runs/full-robot/learning/online-steps';
const read=p=>JSON.parse(readFileSync(p)),hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const source='examples/full-robot/browser-crawl-startup';
const scene=read(`${source}/scene.json`),config=read(`${source}/config.json`),task=read(`${source}/task.json`),planner=read(`${source}/planner.json`);
const period=.02,duration=24;
delete config.motors.target_trajectory;delete config.motion_gate;
config.steps=Math.round(duration/config.step_s);
config.policy.body_feedback.position_world_m.keyframes.length=1;
config.policy.point_feedback.position_world_m.keyframes.length=1;
config.policy.step_reference={sequence:{period_s:period,initial_hold_s:.2,phase_durations_s:[.5,.4,.4,.5,.2],maximum_wait_s:1,qualification_s:.04,lift_m:.005,order:[0,2,1,3],support_offsets_m:[[0,.016,0],[-.018,0,-.003],[0,-.016,0],[.009,0,0]],stance_offsets_m:[[.01,0],[0,0],[.01,0],[0,0]],maximum_speed_m_s:.00125,maximum_yaw_rate_rad_s:.001},command_channels:['command.forward_speed','command.lateral_speed','command.yaw_rate'],initial_yaw_rad:0,minimum_support_force_n:1,placement:planner.placement,bounds:planner.bounds};
scene.controller.inputs.push({name:'command.forward_speed',kind:'LinearVelocity',lower:-.00125,upper:.00125,initial:0},{name:'command.lateral_speed',kind:'LinearVelocity',lower:0,upper:0,initial:0},{name:'command.yaw_rate',kind:'AngularVelocity',lower:-.001,upper:.001,initial:0});
const actions=Array.from({length:Math.round(duration/period)},(_,i)=>[.5,.25,.25,i*period<20?.00125:0,0,0]);
mkdirSync(out,{recursive:true});const write=(n,j)=>writeFileSync(`${out}/${n}`,JSON.stringify(j)+'\n');
write('scene.json',scene);write('config.json',config);write('task.json',task);write('forward-stop.actions.json',actions);
const refined=structuredClone(config);refined.step_s/=2;refined.steps*=2;refined.report_every*=2;refined.implicit.newton.max_iterations=80;write('refined.config.json',refined);
// Reproducible rejected/experimental stance and force-feedback comparisons.
const intermediate=structuredClone(config);
intermediate.policy.step_reference.sequence.stance_offsets_m=[[.018,0],[0,0],[.018,0],[0,0]];
intermediate.policy.step_reference.sequence.support_offsets_m[1]=[-.020,0,-.003];
intermediate.policy.step_reference.sequence.support_offsets_m[3]=[.012,0,0];
write('intermediate-stance.config.json',intermediate);
const preload=structuredClone(intermediate);
preload.policy.step_reference.support_preload={period_s:period,integral_gain_m_per_ns:.005,maximum_extension_m:.004};
write('preload.config.json',preload);
const faster=structuredClone(preload);faster.policy.step_reference.support_preload.integral_gain_m_per_ns=.05;
write('preload-faster.config.json',faster);
write('reverse-stop.actions.json',actions.map(a=>[...a.slice(0,3),-a[3],0,0]));
write('turn-reverse.actions.json',actions.map((a,i)=>{const t=i*period;return [...a.slice(0,3),t<8.4?.00125:t<16.8?0:t<20?-.00125:0,0,t>=8.4&&t<16.8?.001:0];}));
const scheduled=structuredClone(config);
const basePosture=config.policy.step_reference.sequence;
const reversePosture=intermediate.policy.step_reference.sequence;
scheduled.policy.step_reference.sequence.command_postures=[
 {forward_speed_m_s:-.00125,support_offsets_m:reversePosture.support_offsets_m,stance_offsets_m:reversePosture.stance_offsets_m},
 {forward_speed_m_s:0,support_offsets_m:basePosture.support_offsets_m,stance_offsets_m:basePosture.stance_offsets_m}
];
write('scheduled-posture.config.json',scheduled);
const screen=structuredClone(scheduled);screen.policy.step_reference.minimum_planned_support_force_n=.5;
write('scheduled-posture-screen.config.json',screen);
write('forward-reverse-stop.actions.json',actions.map((a,i)=>[...a.slice(0,3),i*period<8.4?.00125:i*period<16.8?-.00125:0,0,0]));
write('manifest.json',{version:1,scope:'Experimental live planar commands through a shared support sequence and CAD inverse kinematics. Bounds are provisional. Ideal feedback and effective-servo omissions remain. Not yet accepted realtime walking.',source_cad_sha256:scene.robot.source.cad_sha256,inputs:Object.fromEntries([`${source}/scene.json`,`${source}/config.json`,`${source}/task.json`,`${source}/planner.json`,'examples/full-robot/prepare_online_steps.mjs'].map(p=>[p,hash(p)]))});
console.log(out);
