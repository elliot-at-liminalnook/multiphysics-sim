// Bind the separately authored/exported CAD revision to the same Rust experiment.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const root='examples/full-robot/measured-actuator-integration';
const base=`${root}/gait-generation`, out=`${base}/full-authority`;
const read=p=>JSON.parse(fs.readFileSync(p));
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
for(const name of ['pilot.spec.json','pilot.settings.json','preparation.json'])
  if(fs.existsSync(`${out}/${name}`))throw Error(`Preserve existing ${name}`);
const receipt=read(`${out}/robot.receipt.json`), exported=read(`${out}/robot.simrobot.receipt.json`);
assert.equal(sha(`${out}/robot.rcad`),receipt.cad_sha256);
assert.equal(exported.cad_sha256,receipt.cad_sha256);
assert.equal(sha(`${out}/robot.simrobot.json`),exported.output_sha256);
const robot=read(`${out}/robot.simrobot.json`);
const parent=read(`${root}/controller-integration/robot-controller.simrobot.json`);
const physical=r=>{const c=structuredClone(r);delete c.actuator_profiles;delete c.source;return c;};
assert.deepEqual(physical(robot),physical(parent),'CAD export changed other physical definitions');
const overridePath=`${root}/controller-integration/comparison-overrides.json`;
for(const override of read(overridePath).overrides){
  assert.equal(override.parameter,'physics.drive_backlash');
  const joints=robot.joints.filter(j=>j.name===override.joint);assert.equal(joints.length,1);
  assert.deepEqual(joints[0].physics.drive_backlash,override.cad_value);
  joints[0].physics.drive_backlash=structuredClone(override.experimental_value);
}
robot.source.experimental_overrides={path:overridePath,sha256:sha(overridePath)};
const spec=read(`${base}/pilot.spec.json`);
assert.equal(spec.scene.robot.source.cad_sha256,receipt.parent_sha256);
// The baseline contains the shared Rust importer's normalized representation.
// Raw CAD export includes legacy fields/default omissions and signed zero spellings.
// Raw parent/new CAD physics already match exactly above. Preserve the actual
// baseline runtime fields; replace only the separately authored profile/source.
const runtimePhysicalBefore=physical(spec.scene.robot);
for(const [name,family] of Object.entries(robot.actuator_profiles.families)){
  const previous=spec.scene.robot.actuator_profiles.families[name];assert(previous);
  for(const key of ['motor','driver','shaft_coordinate'])assert.deepEqual(family[key],previous[key]);
  const a=structuredClone(family.controller),b=structuredClone(previous.controller);
  delete a.gains.limit;delete b.gains.limit;delete a.evidence;delete b.evidence;
  assert.deepEqual(a,b,'Other controller settings changed');
}
for(const [id,binding] of Object.entries(robot.actuator_profiles.bindings)){
  const previous=spec.scene.robot.actuator_profiles.bindings[id];assert(previous);
  for(const key of ['family','physical_unit','deviations','feedback'])assert.deepEqual(binding[key],previous[key]);
}
spec.scene.robot.actuator_profiles=robot.actuator_profiles;
spec.scene.robot.source=robot.source;
assert.deepEqual(physical(spec.scene.robot),runtimePhysicalBefore,'Baseline runtime physics changed');
const rebound=[];
function bind(value,path='config'){
  if(!value||typeof value!=='object')return;
  for(const [key,child] of Object.entries(value)){
    if(key==='expected_cad_sha256'){
      assert.equal(child,receipt.parent_sha256,`Unexpected CAD binding at ${path}.${key}`);
      value[key]=receipt.cad_sha256;rebound.push(`${path}.${key}`);
    }else bind(child,`${path}.${key}`);
  }
}
bind(spec.config);assert.equal(rebound.length,4);
const write=(name,value)=>fs.writeFileSync(`${out}/${name}`,JSON.stringify(value)+'\n');
write('pilot.spec.json',spec);
write('pilot.settings.json',read(`${base}/pilot.settings.json`));
write('preparation.json',{
  scope:'Same provisional motor physics, geometry, gait, commands, world and seed; only CAD controller PWM authority changes from 350 to 1000 permille. No hardware qualification.',
  horizon_s:spec.config.step_s*spec.config.steps,controller_limit_permille:receipt.controller_limit_permille,
  parent_spec_sha256:sha(`${base}/pilot.spec.json`),cad_sha256:receipt.cad_sha256,
  export_sha256:exported.output_sha256,physical_definition_equality_verified:true,
  runtime_physical_definition_reused_from_parent:true,
  import_note:'Raw parent/new CAD physical fields match exactly. The existing Rust-normalized runtime fields are retained, avoiding a second import representation change. The legacy raw world.floor_friction_static field is absent from shared World and remains unmodeled in both cases; ambient/uncertainty defaults retain the baseline representation.',
  rebound,preparation_script_sha256:sha(new URL(import.meta.url)),
  experimental_overrides:robot.source.experimental_overrides,
});
console.log(JSON.stringify({cad_sha256:receipt.cad_sha256,physical_definition_equality_verified:true,rebound}));
