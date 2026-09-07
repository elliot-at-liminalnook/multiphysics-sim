// Sensitivity experiment, not a replacement CAD model or hardware calibration.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/hip-backlash-screen';
const parent='runs/full-robot/learning/hip-timestep';
const inputs={},outputs={};
const hash=b=>createHash('sha256').update(b).digest('hex');
async function read(p){const b=await readFile(p);inputs[p]=hash(b);return JSON.parse(b);}
async function write(p,v){const b=JSON.stringify(v);await writeFile(p,b);outputs[p]=hash(b);}
await mkdir(root,{recursive:true});
const manifest=await read(parent+'/manifest.json');
const scene=await read(manifest.scene);
const joint=scene.robot.joints.find(j=>j.name==='-Y | Hip servo output');
assert.equal(joint.physics.source,'inferred');
const original=joint.physics.backlash;
assert(Math.abs(original-joint.physics.clearance/Math.max(joint.physics.lever,.005))<1e-14);
joint.physics.backlash=0;
const scenePath=root+'/scene.json';await write(scenePath,scene);
const cases=[];
for(const c of manifest.cases){const config=await read(c.config),path=`${root}/${c.name}.config.json`;await write(path,config);cases.push({...c,config:path,reference_capture:null});}
await writeFile(root+'/manifest.json',JSON.stringify({scene:scenePath,inputs,outputs,cases,window_s:manifest.window_s,sample_period_s:manifest.sample_period_s,
 scope:'Single-parameter sensitivity screen. Only -Y hip joint inferred rotational backlash is set to zero. Other motors, bearing clearance, all controller gains, geometry and physics settings are unchanged. This does not assert zero hardware backlash or validate a reduced model.',
 overrides:[{path:'robot.joints[-Y | Hip servo output].physics.backlash',unit:'rad',before:original,after:0,reason:'Test sensitivity to converting radial bearing clearance / COM lever into motor torsional backlash.',provenance:'experimental, uncalibrated',promotion:'Requires an explicit CAD definition distinguishing radial bearing play from measured or estimated drivetrain lost rotation; do not overwrite the baseline from this screen.'}]},null,2));
console.log('Prepared isolated inferred-backlash sensitivity screen.');
