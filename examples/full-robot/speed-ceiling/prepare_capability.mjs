// Assemble a reproducible recipe; mechanics and bounds run in shared Rust.
import fs from 'node:fs';
import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',base='examples/full-robot/fast-wasd';
const configPath=`${base}/braked-5ms.config.json`,scenePath=`${base}/braked-5ms.scene.json`;
const read=p=>JSON.parse(fs.readFileSync(p)),c=read(configPath),scene=read(scenePath);
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const coordinates=c.motors.effective.components.map(m=>m.dof),samples=[];
for(const hip of [0,15,30,45])for(const foot of [-20,-40,-60,-80,-100]) {
 const q=[...c.initial_coordinates];
 for(let leg=0;leg<4;leg++){q[3*leg]=hip*Math.PI/180;q[3*leg+2]=foot*Math.PI/180;}
 samples.push({id:`hip${hip}-foot${foot}`,coordinates:q});
}
const markersPath='examples/full-robot/gait-exploration/workspace-markers.json',markers=read(markersPath);
const recipe={inspection:{independent_coordinates:coordinates,embedding:c.embedding,samples},
 marker_coordinates:Object.fromEntries(markers.markers.map((m,i)=>[m.id,coordinates.slice(3*i,3*i+3)])),
 actuators:Object.fromEntries(c.motors.effective.components.map(m=>[m.dof,m.parameters])),
 directions_world:[0,45,90,135].map(deg=>[Math.cos(deg*Math.PI/180),Math.sin(deg*Math.PI/180),0]),
 duty_factor:.6,support_groups:[[0,1,2,3],[0,2],[1,3]].map(group=>group.map(i=>markers.markers[i].id)),
 support_friction_coefficient:scene.robot.world.floor_friction,
 reference_cycle:{period_s:.8,stride_m:.065*.8,samples:scene.controller.parameters.samples},
 provenance:{baseline_commit:'df669b0',inputs:Object.fromEntries([configPath,scenePath,markersPath,'examples/full-robot/baseline/robot.rcad'].map(p=>[p,sha(p)])),
 actuator_assumptions:c.motors.effective.assumption_reference,
 note:'Fixed base inspection. Body translation does not change inter-link separation or joint point Jacobians. Pose samples are hypotheses, not joint limits. No-load speeds are conditional rate budgets.'}};
fs.mkdirSync(d,{recursive:true});fs.writeFileSync(`${d}/capability-recipe.json`,JSON.stringify(recipe,null,2)+'\n');
console.log({scene:scenePath,markers:markersPath,recipe:`${d}/capability-recipe.json`,samples:samples.length});
