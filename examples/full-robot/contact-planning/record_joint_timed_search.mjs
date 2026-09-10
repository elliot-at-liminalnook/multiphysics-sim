import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import {execFileSync} from 'node:child_process';
const session=Number(process.argv[2]);
assert(Number.isInteger(session)&&session>0,'Pass the live optimizer session ID');
const d='examples/full-robot/contact-planning/';
const identity=path=>({path,bytes:fs.statSync(path).size,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const initial=d+'joint-timed-speed-native-initial.result.json';
const report=JSON.parse(fs.readFileSync(initial));
const prepared=JSON.parse(fs.readFileSync(d+'joint-timed-speed-initial.result.json'));
const canonical=x=>JSON.parse(JSON.stringify(x));
assert(isDeepStrictEqual(canonical(report),canonical(prepared)),'Fresh optimizer initial report differs beyond signed zeros');
const zeroDifferences=(x,y)=>typeof x==='number'&&typeof y==='number'
  ? (x===0&&y===0&&!Object.is(x,y)?1:0)
  : x&&typeof x==='object'?Object.keys(x).reduce((n,k)=>n+zeroDifferences(x[k],y[k]),0):0;
const signed_zero_differences=zeroDifferences(report,prepared);
const original=identity('/Users/elliot/physics-simulator/target/gait-exploration/release/examples/optimize_joint_contact');
assert.equal(original.sha256,'960d66b15b0492bf366b5f0aadfdc71d952b706772efde5811a2671cedc94a46');
const index={source_commit:execFileSync('git',['rev-parse','HEAD']).toString().trim(),recorded_at:new Date().toISOString(),session_id:session,
  optimizer:identity('/Users/elliot/physics-simulator/target/gait-contact-timing/release/examples/optimize_joint_contact'),
  unchanged_comparison_optimizer:original,
  shared_source_build_reference:identity(d+'joint-contact-timing-build.json'),
  optimizer_source:identity('crates/sim-runtime/examples/optimize_joint_contact.rs'),
  cargo_lock:identity('Cargo.lock'),recorder:identity(d+'record_joint_timed_search.mjs'),
  build_command:'CARGO_TARGET_DIR=/Users/elliot/physics-simulator/target/gait-contact-timing CARGO_BUILD_JOBS=2 /Users/elliot/.cargo/bin/cargo build --release -p sim-runtime --example optimize_joint_contact',
  build_log:identity(d+'joint-timed-optimizer-build.log'),
  rustc:execFileSync('/Users/elliot/.cargo/bin/rustc',['-Vv']).toString(),cargo:execFileSync('/Users/elliot/.cargo/bin/cargo',['-V']).toString(),
  inputs:['runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json','examples/full-robot/gait-exploration/workspace-markers.json',d+'joint-timed-speed.recipe.json'].map(identity),
  initial:identity(initial),initial_log:identity(d+'joint-timed-speed-native-initial.log'),
  initial_report_matches_prepared_recipe_after_signed_zero_canonicalization:true,signed_zero_differences,
  arguments:['runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json','examples/full-robot/gait-exploration/workspace-markers.json',d+'joint-timed-speed.recipe.json','--analytic-forces'],
  stdout:d+'joint-timed-speed.result.json',stderr:d+'joint-timed-speed.log',
  scope:'Live 8000-evaluation search of the contact-relative force timing representation, from the same 0.2117102645 m/s motion and broader-height bounds as joint-height-speed. Strict initial contact event ordering is an explicit local domain restriction. No completed result, speed gain, physical ceiling or runtime/browser qualification is claimed.'};
fs.writeFileSync(d+'joint-timed-speed-launch.json',JSON.stringify(index,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({session_id:session,optimizer_sha256:index.optimizer.sha256,initial_report_matches_after_signed_zero_canonicalization:true,signed_zero_differences}));
