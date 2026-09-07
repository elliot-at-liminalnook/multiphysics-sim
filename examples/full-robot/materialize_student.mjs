import {readFileSync,writeFileSync} from 'node:fs';
import assert from 'node:assert/strict';
const fit=process.argv[2]||'runs/full-robot/learning/student-distillation/multi-fit';
const root='examples/full-robot/student-distillation';const read=p=>JSON.parse(readFileSync(p));
assert.deepEqual(read(`${fit}/experiment.json`),read(`${root}/experiment.json`));
const policy=read(`${fit}/policy.json`),config=read(`${root}/initial.config.json`);
config.policy.neural_residual=policy;config.policy.feedback_observations=false;config.policy.task_observations.floor_forces=false;
const write=(name,v)=>writeFileSync(`${root}/${name}.json`,JSON.stringify(v)+'\n');
write('policy',policy);write('fit',read(`${fit}/fit.json`));write('validation',read(`${fit}/validation.json`));
write('short.config',config);config.steps=Math.round(60/config.step_s);write('config',config);
write('observation-boundary',{version:1,deployable:false,cad_declared_sensors:read(`${root}/scene.json`).robot.sensors,network_features:policy.features,
 base_motor_feedback:['joint references','ideal joint angle','declared tracking gain'],
 excluded_from_motor_policy:['world body position and linear velocity','foot marker positions/velocities','contact forces','body/point correction suggestions'],
 remaining_privileged_dependencies:['online planner observes ideal body/foot state','contact forces qualify planner lift/landing and preload','joint velocity is ideal; causal encoder differentiation is not implemented','body gravity direction/angular rate are ideal; IMU availability, mounting, calibration and estimator unconfirmed'],
 required_before_hardware:['declare actual sensors and provenance in CAD','bind sensor frames, sample clocks, delays and noise','supply causal state/contact estimates to the planner','validate the complete policy observation path against hardware']});
console.log(root);
