// Bind an explicitly authored/exported CAD profile scenario to an existing search.
// All geometry/physics derivation and runtime profile validation remain shared.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';

const [parentSpecPath,parentExportPath,out,powerSelectionPath] = process.argv.slice(2);
if (!out) throw Error('Usage: prepare_profile_search.mjs parent-spec parent-export scenario-directory [power-selection.json]');
const read = p => JSON.parse(fs.readFileSync(p));
const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
for (const name of ['pilot.spec.json','pilot.settings.json','preparation.json'])
  if (fs.existsSync(`${out}/${name}`)) throw Error(`Preserve ${out}/${name}`);
const spec = read(parentSpecPath), parent = read(parentExportPath);
const receipt = read(`${out}/robot.receipt.json`), exported = read(`${out}/robot.simrobot.receipt.json`);
assert.equal(sha(receipt.parent),receipt.parent_sha256);
assert.equal(sha(receipt.profiles),receipt.profiles_sha256);
assert.equal(sha(`${out}/robot.rcad`),receipt.cad_sha256);
assert.equal(exported.cad_sha256,receipt.cad_sha256);
assert.equal(sha(`${out}/robot.simrobot.json`),exported.output_sha256);
assert.equal(spec.scene.robot.source.cad_sha256,receipt.parent_sha256);
assert.equal(parent.source.cad_sha256,receipt.parent_sha256);
const robot = read(`${out}/robot.simrobot.json`);
assert.deepEqual(robot.actuator_profiles,read(receipt.profiles));
const physical = r => {const c=structuredClone(r);delete c.actuator_profiles;delete c.source;return c;};
assert.deepEqual(physical(robot),physical(parent),'Profile scenario changed raw physical export');
const before = physical(spec.scene.robot);
// Retain the exact parent runtime representation and its recorded external overrides.
const overrides = spec.scene.robot.source.experimental_overrides;
if (overrides) assert.equal(sha(overrides.path),overrides.sha256);
spec.scene.robot.actuator_profiles = robot.actuator_profiles;
spec.scene.robot.source = {...robot.source};
if (overrides) spec.scene.robot.source.experimental_overrides = overrides;
assert.deepEqual(physical(spec.scene.robot),before);
const rebound = [];
function bind(value,path='config') {
  if (!value || typeof value !== 'object') return;
  for (const [key,child] of Object.entries(value)) {
    if (key === 'expected_cad_sha256') {
      assert.equal(child,receipt.parent_sha256);
      value[key]=receipt.cad_sha256;rebound.push(`${path}.${key}`);
    } else bind(child,`${path}.${key}`);
  }
}
bind(spec.config);
assert(rebound.length>0,'Missing CAD runtime binding');
if (robot.actuator_profiles.power) {
  assert(powerSelectionPath,'CAD power requires an explicit numerical power-selection file');
  const selection = read(powerSelectionPath);
  assert.deepEqual(Object.keys(selection),['residual_scales']);
  assert(Array.isArray(selection.residual_scales) && selection.residual_scales.length===4 &&
    selection.residual_scales.every(v=>Number.isFinite(v)&&v>0));
  spec.config.motors.power=selection;
} else {
  assert(!powerSelectionPath && !spec.config.motors.power,'Power selection requires a CAD power tree');
}
const write = (name,value) => fs.writeFileSync(`${out}/${name}`,JSON.stringify(value)+'\n');
write('pilot.spec.json',spec);
write('pilot.settings.json',{seed:spec.seed,initial_design:3,acquisition_starts:4,maximum_training_rows:64});
write('preparation.json',{
  scope:'CAD profile sensitivity scenario; identical imported geometry/mechanics, world, gait, initial state and seed. Not hardware calibration.',
  parent_spec:{path:parentSpecPath,sha256:sha(parentSpecPath)},
  parent_export:{path:parentExportPath,sha256:sha(parentExportPath)},
  cad_sha256:receipt.cad_sha256,export_sha256:exported.output_sha256,
  profiles_sha256:receipt.profiles_sha256,physical_definition_equality_verified:true,
  runtime_physical_definition_reused_from_parent:true,rebound,
  horizon_s:spec.config.steps*spec.config.step_s,
  power_selection:powerSelectionPath ? {path:powerSelectionPath,sha256:sha(powerSelectionPath)} : null,
  experimental_overrides:overrides,preparation_script_sha256:sha(new URL(import.meta.url)),
});
console.log(JSON.stringify({cad_sha256:receipt.cad_sha256,rebound}));
