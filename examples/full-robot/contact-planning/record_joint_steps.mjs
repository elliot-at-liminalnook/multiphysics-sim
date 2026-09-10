import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {gzipSync,gunzipSync} from 'node:zlib';
import {execFileSync} from 'node:child_process';
const root='examples/full-robot/contact-planning/';
const prefix=root+'joint-step-';
const read=s=>JSON.parse(fs.readFileSync(prefix+s));
const hash=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
const evidence=path=>({path,bytes:fs.statSync(path).size,sha256:hash(fs.readFileSync(path))});
const source=JSON.parse(fs.readFileSync(root+'joint-mesh-command-speed.recipe.json'));
const expanded=read('speed.recipe.json'),start=read('conic-speed.recipe.json');
assert.deepEqual({...expanded.robot,uniform_samples:source.robot.uniform_samples,
  additional_phases:source.robot.additional_phases},source.robot);
assert.deepEqual(start.robot,expanded.robot);
assert.deepEqual(start.variables,expanded.variables);
const command=read('command-map.result.json'),force=read('force-jacobian.result.json'),conic=read('conic.result.json'),smoke=read('smoke.result.json');
assert.equal(expanded.variables.length,884);
assert.equal(expanded.variables.filter(v=>v.decision.kind==='additional_force').length,366);
assert.equal(command.reference_report.sampled_feasible,true);
assert(command.cases.every(c=>c.baseline_cache_byte_equal&&c.bounded_probe_errors.every(e=>e<=1e-8)));
assert(force.cases.every(c=>c.checked_force_columns===732&&c.fallback_columns===0&&c.max_scaled_error<=force.scaled_error_tolerance));
assert.equal(conic.search.status,'Solved');assert.equal(conic.report.sampled_feasible,true);
assert.equal(conic.force_box_violation_n,0);assert.equal(conic.report.maximum_cone_violation_n,0);
assert(conic.independent_affine_error<=1e-8&&conic.independent_servo_command_affine_error<=1e-8);
const legacy=fs.readFileSync(prefix+'legacy-conic.result.json');
assert(legacy.equals(fs.readFileSync(root+'joint-mesh-command-best.result.json')));
assert.equal(smoke.model_evaluations,4);assert.equal(smoke.model_budget_exhausted,true);
assert.equal(smoke.report.sampled_feasible,true);assert.equal(smoke.search.callback_panicked,false);
const archives=[];
for(const suffix of ['command-map.result.json','force-jacobian.result.json','conic.result.json','legacy-conic.result.json','smoke.result.json']) {
  const path=prefix+suffix,bytes=fs.readFileSync(path),archive=path+'.gz';
  fs.writeFileSync(archive,gzipSync(bytes,{level:9}),{flag:'wx'});
  assert(gunzipSync(fs.readFileSync(archive)).equals(bytes));
  archives.push({uncompressed:evidence(path),compressed:evidence(archive)});
}
const code=[
  'crates/sim-runtime/src/contact_planning/joint.rs','crates/sim-runtime/src/contact_planning/joint_steps.rs',
  'crates/sim-runtime/src/contact_planning/joint_conic.rs','crates/sim-runtime/src/contact_planning/joint_ipopt.rs',
  'crates/sim-runtime/src/contact_planning/joint_timing.rs',
  ...['expand_joint_contact_cycle','optimize_joint_ipopt_steps','optimize_joint_ipopt',
    'audit_joint_servo_command_map','audit_joint_force_jacobian','audit_joint_force_basis',
    'audit_joint_force_cache','audit_joint_support_space','align_joint_contact_forces']
    .map(name=>'crates/sim-runtime/examples/'+name+'.rs'),
  root+'record_joint_steps.mjs',
];
const binaries=['expand_joint_contact_cycle','audit_joint_servo_command_map','audit_joint_force_jacobian','solve_joint_force_cones','optimize_joint_ipopt_steps']
  .map(name=>evidence('/Users/elliot/physics-simulator/target/gait-exploration/release/examples/'+name));
const manifest={version:1,base_commit:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),
  code_files:code.map(evidence),binaries,archives,
  inputs:[root+'joint-mesh-command-speed.recipe.json',root+'joint-mesh-command-best.recipe.json',
    root+'joint-mesh-command-best.result.json',prefix+'speed.recipe.json',prefix+'conic-speed.recipe.json',
    prefix+'speed.search.json',prefix+'smoke.search.json',
    'runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json',
    'examples/full-robot/gait-exploration/workspace-markers.json','Cargo.lock'].map(evidence),
  logs:['tests.log','build.log','build-initial.log','examples-check.log','expansion.log','command-map.log',
    'force-jacobian.log','conic.log','legacy-conic.log','smoke.log'].map(s=>evidence(prefix+s)),
  verification:{planner_tests:12,all_runtime_examples_checked:true,legacy_conic_byte_identical:true,
    command_cases:command.cases,force_cases:force.cases,
    conic:{status:conic.search.status,force_variables:conic.force_variables,balance_rows:conic.balance_rows,
      servo_command_rows:conic.servo_command_rows,feasible:conic.report.sampled_feasible,
      independent_affine_error:conic.independent_affine_error,independent_servo_command_affine_error:conic.independent_servo_command_affine_error},
    smoke:{models:smoke.model_evaluations,model_budget_exhausted:true,native_status:smoke.search.native_status,
      last_callback_error:smoke.search.last_callback_error,feasible:smoke.report.sampled_feasible,
      variables:884,frames:smoke.report.motion_report.frames.length,jacobian_nonzeros:smoke.jacobian_nonzeros,
      scope:'Four-model interface/structure check intentionally exhausts the CAD budget; not an NLP convergence result.'}},
  scene_archive_index:'examples/full-robot/speed-ceiling/evidence-v8-index.json',
  scope:'Per-stance joint-force integration and native search initialization. All physical properties and gates retained. Archives contain complete terminal audit results. No faster executed gait or physical-speed maximum is established.'};
fs.writeFileSync(prefix+'evidence.json',JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({archives:archives.length,compressed_bytes:archives.reduce((n,a)=>n+a.compressed.bytes,0),checks_passed:true}));
