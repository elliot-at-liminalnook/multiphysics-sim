import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
const d='examples/full-robot/contact-planning/';
const identity=path=>({path,bytes:fs.statSync(path).size,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const sources=['Cargo.lock','crates/sim-runtime/src/contact_planning.rs','crates/sim-runtime/src/contact_planning/joint.rs','crates/sim-runtime/src/contact_planning/joint_timing.rs','crates/sim-runtime/examples/bind_joint_contact_timing.rs','crates/sim-runtime/examples/audit_joint_force_jacobian.rs','crates/sim-runtime/examples/compile_contact_reference.rs',d+'prepare_joint_contact_timing.mjs',d+'record_joint_contact_timing.mjs'];
const archive=d+'joint-contact-timing-sources.tar.gz';
assert(!fs.existsSync(archive),'Source archive already exists');
execFileSync('tar',['-czf',archive,...sources]);
const native='/Users/elliot/physics-simulator/target/gait-exploration/release/examples/';
const optimizer=identity(native+'optimize_joint_contact');
assert.equal(optimizer.sha256,'960d66b15b0492bf366b5f0aadfdc71d952b706772efde5811a2671cedc94a46','Running optimizer binary changed');
const artifacts=fs.readdirSync(d).filter(n=>
  n.startsWith('joint-contact-timing-') && n!=='joint-contact-timing-build.json' && n!=='joint-contact-timing-sources.tar.gz'
  || ['joint-contact-timing.log','joint-contact-timing.result.json','CONTACT_TIMING.md','README.md','joint-final-support-summary.json','joint-body8-speed.log','joint-body8-speed.result.json','joint-body8-speed.summary.json','joint-timed-speed.recipe.json','joint-timed-speed-initial.result.json','joint-timed-phase-probe.recipe.json'].includes(n)
  || /^joint-multistart-(constant_mean|reference)-d0\.75-final-/.test(n)
).sort().map(n=>d+n);
const index={base_commit:execFileSync('git',['rev-parse','HEAD']).toString().trim(),source_archive:identity(archive),sources:sources.map(identity),binaries:['bind_joint_contact_timing','audit_joint_force_jacobian','compile_contact_reference'].map(n=>identity(native+n)),unchanged_running_optimizer:optimizer,optimizer_build_reference:identity(d+'joint-derivative-build.json'),support_audit_binary:identity(native+'audit_joint_support_space'),support_build_reference:identity(d+'joint-support-space-evidence.json'),inputs:['runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json','examples/full-robot/gait-exploration/workspace-markers.json',d+'joint-height-speed.recipe.json',d+'joint-height-speed-initial.result.json',d+'joint-body8-speed-launch.json',d+'joint-multistart-constant_mean-d0.75.result.json',d+'joint-multistart-reference-d0.75.result.json'].map(identity),artifacts:artifacts.map(identity),commands:[
'CARGO_TARGET_DIR=/Users/elliot/physics-simulator/target/gait-exploration cargo test -p sim-runtime --lib contact_planning -- --nocapture',
'CARGO_TARGET_DIR=/Users/elliot/physics-simulator/target/gait-exploration cargo build --release -p sim-runtime --example bind_joint_contact_timing --example audit_joint_force_jacobian --example compile_contact_reference',
'bind_joint_contact_timing runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json examples/full-robot/gait-exploration/workspace-markers.json '+d+'joint-height-speed.recipe.json',
'node '+d+'prepare_joint_contact_timing.mjs',
'audit_joint_force_jacobian runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json examples/full-robot/gait-exploration/workspace-markers.json '+d+'joint-timed-phase-probe.recipe.json'
],scope:'Shared contact-relative force-knot parameterization and focused analytic/CAD/derivative checks. Final body8 search and two instantaneous support audits retained. Two older optimizer processes continue separately and their live output/logs are excluded. The timed speed recipe is prepared but has no optimization result yet; no physical speed ceiling or new runtime/browser gait is claimed.'};
fs.writeFileSync(d+'joint-contact-timing-build.json',JSON.stringify(index,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({source_files:sources.length,artifacts:artifacts.length,unchanged_optimizer_sha256:optimizer.sha256}));
