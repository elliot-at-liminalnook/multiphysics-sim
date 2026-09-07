// Provisional sensor-limited student: replace privileged motor feedback with a network.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [trainPath='runs/full-robot/learning/neural-teacher/train.native.json',validationPath='runs/full-robot/learning/neural-teacher/heldout.native.json',output='examples/full-robot/student-distillation',additionalTrainPath='runs/full-robot/learning/neural-teacher/sustained-80.native.json']=process.argv.slice(2);
const read=p=>JSON.parse(readFileSync(p));
const training=read(trainPath),validation=read(validationPath);assert(training.completed&&validation.completed);
const additional=read(additionalTrainPath);assert(additional.completed);assert.deepEqual(additional.recording.scene.robot,training.recording.scene.robot);
assert.notDeepEqual(training.recording.input_events,validation.recording.input_events);
const scene=read('examples/full-robot/neural-teacher/scene.json'),config=read('examples/full-robot/neural-teacher/short.config.json'),task=read('examples/full-robot/neural-teacher/task.json');
const outputs=config.policy.neural_residual.outputs.map(o=>({...o,scale:.05}));
const features=[];const add=(source,kind,scale,subtract=null,center=0)=>features.push({source,kind,scale,subtract,center,clip:5});
const initial=training.frames.find(f=>f.policy)?.policy.observations;
for(const o of outputs){const joint=o.target.replace(/\.target$/,'');add(`${joint}.angle`,'Angle',.02,`${joint}.reference`);add(`${joint}.reference`,'Angle',.2,null,initial[`${joint}.angle`]);add(`${joint}.angular_velocity`,'AngularVelocity',.1);}
for(const axis of ['x','y','z']){add(`body.gravity_direction.${axis}`,'Dimensionless',.05,null,axis==='z'?-1:0);add(`body.angular_velocity.${axis}`,'AngularVelocity',.1);}
add('command.forward_speed','LinearVelocity',.00125);add('command.lateral_speed','LinearVelocity',.00125);add('command.yaw_rate','AngularVelocity',.001);
const hidden=64;
const network={version:1,features,outputs,layers:[{weights:Array.from({length:hidden},(_,i)=>features.map((_,j)=>.1*Math.sin((i+1)*(j+3)))),biases:Array(hidden).fill(0)},{weights:outputs.map(()=>Array(hidden).fill(0)),biases:outputs.map(()=>0)}]};
const sensors=[...new Map(features.flatMap(f=>[[f.source,{name:f.source,kind:f.kind}],...(f.subtract?[[f.subtract,{name:f.subtract,kind:f.kind}]]:[])])).values()];
const dataset=c=>({version:1,sensors,actuators:outputs.map(o=>({name:o.target,kind:o.kind})),samples:c.frames.filter(f=>f.policy).map(f=>({observations:sensors.map(s=>{const v=f.policy.observations[s.name];assert(Number.isFinite(v));return v;}),actions:outputs.map(o=>{const joint=o.target.replace(/\.target$/,'');const p=f.policy.observations;return f.policy.targets[o.target]-p[`${joint}.reference`]-.5*(p[`${joint}.reference`]-p[`${joint}.angle`]);})}))});
scene.controller.sources={entry:'student.rhai',files:{'student.rhai':'fn control(t, sensors, commands, state) { for name in commands.keys() { let joint = name.sub_string(0, name.len()-7); let r = sensors[joint+".reference"]; commands[name] = r + sensors["command.tracking_gain"] * (r - sensors[joint+".angle"]); } #{commands:commands,state:state} }'}};
config.policy.neural_residual=network;config.implicit.newton.max_iterations=80;
// The planner still uses ideal contact/kinematics; these motor-feedback channels
// are intentionally absent from the student network and its Rhai base controller.
for(let i=0;i<outputs.length;i++)task.rewards.find(r=>r.name===`motor.${i}.residual_size`).scale=.05;
mkdirSync(output,{recursive:true});const write=(name,v)=>writeFileSync(`${output}/${name}.json`,JSON.stringify(v)+'\n');
write('initial.policy',network);write('scene',scene);write('initial.config',config);write('task',task);
const trainData=dataset(training);trainData.samples.push(...dataset(additional).samples);
write('experiment',{version:1,network,training:trainData,validation:dataset(validation),optimizer:{epochs:1000,learning_rate:.0005,batch_size:64}});
write('manifest',{version:1,inputs:[trainPath,additionalTrainPath,validationPath,'examples/full-robot/neural-teacher/scene.json','examples/full-robot/neural-teacher/short.config.json','examples/full-robot/neural-teacher/task.json','examples/full-robot/prepare_student_distillation.mjs'].map(path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')})),training_samples:trainData.samples.length,validation_samples:dataset(validation).samples.length,
 source_cad_sha256:scene.robot.source.cad_sha256,declared_cad_sensors:scene.robot.sensors.length,
 scope:'Imitate complete teacher motor feedback beyond reference plus local joint tracking. Student network excludes ideal body translation, foot geometry and contact forces. Proposed encoder/IMU observations still come from ideal simulation; actual sensors, rate/noise/delay and estimator unconfirmed. Upstream step planner remains privileged. Not hardware deployable.'});
console.log(output);
