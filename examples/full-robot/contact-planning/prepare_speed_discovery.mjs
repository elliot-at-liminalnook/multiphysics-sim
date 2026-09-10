// Build a continuous-distance task using an existing CAD-derived Rust scene.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning';
const [source='runs/contact-planning/diagonal21-screen',id='diagonal21']=process.argv.slice(2);
assert(/^[a-z0-9-]+$/.test(id));
const prefix='runs/contact-planning/speed-discovery-'+id;
const read=p=>JSON.parse(fs.readFileSync(p));
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const scene=read(source+'.scene.json'),config=read(source+'.config.json');
const sourceActions=read(source+'.actions.json');
const speedIndex=scene.controller.inputs.findIndex(ch=>ch.name==='command.forward_speed');
const gainIndex=scene.controller.inputs.findIndex(ch=>ch.name==='command.tracking_gain');
assert(speedIndex>=0&&gainIndex>=0);
const duration=20,speed=sourceActions.reduce((v,row)=>Math.max(v,Math.abs(row[speedIndex])),0);
const gain=scene.controller.inputs[gainIndex].initial;
assert(speed>0&&Number.isFinite(gain));
scene.duration_s=duration;config.steps=Math.round(duration/config.step_s);
assert.equal(config.step_s,.000625);
const actions=Array.from({length:Math.round(duration/scene.period_s)},(_,i)=>scene.controller.inputs.map(ch=>
  ch.name==='command.forward_speed'?speed:ch.name==='command.packet_sequence'?i+1:ch.name==='command.yaw_rate'||ch.name==='command.lateral_speed'?0:ch.initial));
for(const [suffix,value] of [['scene',scene],['config',config],['actions',actions]])write(prefix+'.'+suffix+'.json',value);
const task=read('examples/full-robot/fast-wasd/task.json');
task.rewards=[];task.survival_reward_per_s=0;task.termination_penalty=0;
task.observations.push({name:'body.floor_force.z',source:{kind:'floor_force',link:'Robot | Chassis and hip mounts',axis:'z'}});
task.termination_bounds=[{observation:'body.up.z',lower:0,upper:1.000000001},{observation:'body.floor_force.z',lower:0,upper:0}];
if(fs.existsSync(d+'/speed-discovery-task.json'))assert.deepEqual(read(d+'/speed-discovery-task.json'),task);
else write(d+'/speed-discovery-task.json',task);
const spec=read(d+'/bayesian-human20-pilot-v2.spec.json');
spec.source_prefix=prefix;spec.task=d+'/speed-discovery-task.json';
spec.output_directory='runs/bayesian-speed-only-'+id;spec.evaluation_profile='speed20';spec.baseline_replay=false;
spec.baseline_values=[speed,gain,1];spec.initial_design_count=8;spec.comparison_evaluations=16;
spec.problem.parameters[0].bounds=[.05,.6];spec.problem.parameters[1].bounds=[0,3];spec.problem.parameters[2].bounds=[0,3];
spec.problem.objective_name='negative net horizontal chassis displacement divided by full trial duration';
spec.problem.constraints=[{name:'fall observed (chassis ground contact or overturning)',unit:'1',scale:1}];
spec.scope='Continuous 20-second travel. Optimize net distance/time without falling. No slip, lift, requested-speed tracking, heading, steering or stopping rejection. Physical CAD, actuator and runtime model preserved. Parameter ranges are initial search windows to expand, not physical ceilings. Fall geometry is sampled; timestep and longer-distance confirmation follow promising results.';
write(d+'/bayesian-speed-only-'+id+'.spec.json',spec);
write(d+'/speed-discovery-'+id+'-preparation.json',{source,source_inputs:['scene','config','actions'].map(s=>({path:source+'.'+s+'.json',sha256:hash(source+'.'+s+'.json')})),
  prepared_inputs:['scene','config','actions'].map(s=>({path:prefix+'.'+s+'.json',sha256:hash(prefix+'.'+s+'.json')})),
  task:{path:spec.task,sha256:hash(spec.task)},code:{path:import.meta.filename,sha256:hash(import.meta.filename)},
  changes:'Only duration, step count and action schedule changed in source simulation inputs. Task rewards removed and task bounds replaced by overturn/chassis-floor-contact detection. CAD physical model and actuator values unchanged.'});
