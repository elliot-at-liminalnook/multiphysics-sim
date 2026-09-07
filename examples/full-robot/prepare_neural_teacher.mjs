// Initial small neural residual policy, using shared Rust sampled inference.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
const output=process.argv[2]||'examples/full-robot/neural-teacher';
const read=p=>JSON.parse(readFileSync(p));
const source='examples/full-robot/browser-residual-policy';
const scene=read(`${source}/scene.json`),config=read(`${source}/short.config.json`),task=read(`${source}/task.json`);
const bindings=read(`${source}/learning.json`).policy_action_bindings;
for(const input of scene.controller.inputs.filter(c=>c.name.startsWith('residual.')))input.lower=input.upper=input.initial=0;
const features=[];
const add=(source,kind,scale,subtract=null,center=0)=>features.push({source,subtract,kind,center,scale,clip:5});
for(const b of bindings){const joint=b.coordinate.replace(/^joint\./,'');add(`${joint}.angle`,'Angle',.02,`${joint}.reference`);add(`${joint}.angular_velocity`,'AngularVelocity',.1);}
for(const axis of ['x','y']){
 add(`body.gravity_direction.${axis}`,'Dimensionless',.05);
 add(`body.linear_velocity.${axis}`,'LinearVelocity',.01);
 add(`body.angular_velocity.${axis}`,'AngularVelocity',.1);
}
add('command.forward_speed','LinearVelocity',.00125);add('command.yaw_rate','AngularVelocity',.001);
for(const marker of config.policy.task_observations.markers)add(`marker.${marker.id}.floor_force_world.z`,'Force',20);
const hidden=8;
config.policy.neural_residual={version:1,features,outputs:bindings.map(b=>({target:b.target,kind:'Angle',scale:.001})),layers:[
 {weights:Array.from({length:hidden},(_,i)=>features.map((_,j)=>.1*Math.sin((i+1)*(j+3)))),biases:Array(hidden).fill(0)},
 {weights:bindings.map(()=>Array(hidden).fill(0)),biases:bindings.map(()=>0)},
]};
for(const [i,b] of bindings.entries()){
 const observation=task.observations.find(o=>o.name===`motor.${i}.residual`);
 observation.source={kind:'neural_correction',actuator:b.target};
 // Normalize the penalty to the new explicitly smaller learned-action scale.
 task.rewards.find(r=>r.name===`motor.${i}.residual_size`).scale=.001;
}
const train=read(`${source}/forward-reverse.actions.json`);
const heldout=read('examples/full-robot/browser-reversal/reverse-forward.actions.json').map(a=>[...a,...bindings.map(()=>0)]);
mkdirSync(output,{recursive:true});
const write=(name,v)=>writeFileSync(`${output}/${name}.json`,JSON.stringify(v)+'\n');
write('scene',scene);write('initial.config',config);write('task',task);write('train.actions',train);write('heldout.actions',heldout);
write('experiment',{version:1,scene,config,task,actions:train,environment_seed:0,search:{seed:417,iterations:4,perturbation:.01}});
write('manifest',{version:1,stage:'initial teacher policy search',features:features.length,hidden,outputs:bindings.length,
 source_cad_sha256:scene.robot.source.cad_sha256,
 inputs:Object.fromEntries([`${source}/scene.json`,`${source}/short.config.json`,`${source}/task.json`,`${source}/learning.json`,`${source}/forward-reverse.actions.json`,'examples/full-robot/browser-reversal/reverse-forward.actions.json','examples/full-robot/prepare_neural_teacher.mjs'].map(p=>[p,createHash('sha256').update(readFileSync(p)).digest('hex')])),
 scope:'Zero output-layer initialization preserves baseline targets; all network weights are trainable. Normalization scales are numerical design choices, not physical calibration. Ideal teacher features and ±0.001 rad output bounds; arbitrary continuous corrections unvalidated. Held-out reverse-first commands are excluded from training.'});
console.log(output);
