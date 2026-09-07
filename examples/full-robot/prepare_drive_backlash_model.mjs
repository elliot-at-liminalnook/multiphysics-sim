// Explicit v4 experiment: no additional lost rotation at rigid drive
// connections. Motor gearbox parameters remain authored, uncalibrated inputs.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/drive-backlash-definition';
const inputs={},outputs={};
const hash=b=>createHash('sha256').update(b).digest('hex');
async function read(p){const b=await readFile(p);inputs[p]=hash(b);return JSON.parse(b);}
async function write(p,v){const b=JSON.stringify(v);await writeFile(p,b);outputs[p]=hash(b);}
await mkdir(root,{recursive:true});
const parent=await read('runs/full-robot/learning/analytic-positions/manifest.json');
const scene=await read(parent.scene),original=structuredClone(scene);
scene.robot.version=4;
const overrides=[];
for(const motor of scene.robot.motors){
 const joint=scene.robot.joints.find(j=>j.name===motor.joint);assert(joint);
 // The broad joint source can be "declared" after an unrelated override.
 // Verify the actual number still equals the legacy geometric heuristic.
 assert(Math.abs(joint.physics.backlash-joint.physics.clearance/Math.max(joint.physics.lever,.005))<1e-14);
 assert(!joint.physics.identified);
 assert.equal(motor.gearbox.backlash_rad,0);
 joint.physics.drive_backlash={width_rad:0,provenance:'estimated',reference:'Experimental ideal rigid drive connection: no additional torsional lost motion. Bearing clearance is retained separately; hardware reversal measurements and loaded attachment validation are pending.'};
 overrides.push({joint:joint.name,legacy_joint_source:joint.physics.source,legacy_bearing_angle_rad:joint.physics.backlash,drive_backlash:joint.physics.drive_backlash});
}
assert.equal(overrides.length,12);
const scenePath=root+'/scene.json';await write(scenePath,scene);
const config=await read('runs/full-robot/learning/analytic-positions/base.config.json');
const cases=[];
for(const [name,step] of [['2000us',.002],['1000us',.001],['500us',.0005],['250us',.00025],['125us',.000125],['62p5us',.0000625]]){
 const c={...structuredClone(config),step_s:step,steps:Math.round(2.8/step),report_every:Math.round(.01/step)};
 const path=`${root}/${name}.config.json`;await write(path,c);cases.push({name,step_s:step,config:path});
}
// A v4 compatibility case labels old heuristic numbers explicitly as estimates.
const compatibility=structuredClone(original);compatibility.robot.version=4;
for(const motor of compatibility.robot.motors){const j=compatibility.robot.joints.find(j=>j.name===motor.joint);j.physics.drive_backlash={width_rad:j.physics.backlash,provenance:'estimated',reference:'Legacy heuristic reproduced for migration equivalence only; not a measured drive property'};}
await write(root+'/compatibility.scene.json',compatibility);
inputs[import.meta.filename]=hash(await readFile(import.meta.filename));
await writeFile(root+'/manifest.json',JSON.stringify({scene:scenePath,source_scene:parent.scene,inputs,outputs,cases,overrides,
 scope:'All twelve drive connections explicitly idealized as zero additional backlash. Existing motor gearbox values, bearing clearance, controller, world and other physical values are unchanged. Unknown hardware drivetrain backlash is not calibrated by this experiment. Full 2.8 s motions; same policy/firmware sampling across nominal timestep cases.',
 promotion:'Keep this an estimated experiment; record accepted drive-connection properties through CAD set_joint_physics(drive_backlash=...). Hardware backlash must distinguish motor gearbox and external attachment contributions. No duplicated total backlash.'},null,2));
console.log('Prepared v4 drive-definition experiment and explicit legacy-equivalence scene.');
