import fs from 'node:fs';
import crypto from 'node:crypto';
import {execFileSync} from 'node:child_process';
const prefix='examples/full-robot/contact-planning/';
const evidence=path=>({path,bytes:fs.statSync(path).size,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const code=[
  'crates/sim-domain-control/src/contact_phase.rs',
  'crates/sim-domain-control/src/contact_phase/sequence.rs',
  'crates/sim-domain-control/src/contact_phase/sequence_tests.rs',
  'crates/sim-runtime/src/contact_planning.rs',
  'crates/sim-runtime/src/contact_planning/joint.rs',
  'crates/sim-runtime/src/contact_planning/joint_timing.rs',
  'crates/sim-runtime/examples/evaluate_contact_sequence.rs',
  'crates/sim-script/tests/contact_phase.rs',
  prefix+'check_contact_sequences.mjs',prefix+'record_contact_sequences.mjs',
];
const files=['single.recipe.json','double.raw.json','double.recipe.json','single.result.json',
  'double.result.json','equivalence.json','replay.json','tests.log','planner-tests.log','build.log',
  'single.error.log','double.error.log'].map(s=>prefix+'contact-sequence-'+s);
const summary=JSON.parse(fs.readFileSync(prefix+'contact-sequence-equivalence.json'));
if(!summary.passed)throw Error('equivalence must pass');
const manifest={version:1,base_commit:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),
  code_files:code.map(evidence),artifacts:files.map(evidence),
  binary:evidence('/Users/elliot/physics-simulator/target/gait-exploration/release/examples/evaluate_contact_sequence'),
  inputs:[
    'examples/full-robot/baseline/robot.rcad',
    'runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json',
    'examples/full-robot/gait-exploration/workspace-markers.json',
    prefix+'joint-mesh-command-best.recipe.json',
    'Cargo.lock',
  ].map(evidence),
  scene_archive_index:'examples/full-robot/speed-ceiling/evidence-v8-index.json',
  completed_commands:[
    {command:'cargo test -p sim-domain-control -p sim-script',exit_code:0},
    {command:'cargo test -p sim-runtime --lib contact_planning --features native-ipopt,conic',exit_code:0},
    {command:'cargo build -p sim-runtime --release --example evaluate_contact_sequence',exit_code:0},
    {command:'evaluate_contact_sequence --repeat contact-sequence-single.recipe.json 2',exit_code:0},
    {command:'evaluate_contact_sequence scene markers contact-sequence-single.recipe.json',exit_code:0},
    {command:'evaluate_contact_sequence scene markers contact-sequence-double.recipe.json',exit_code:0},
    {command:'node examples/full-robot/contact-planning/check_contact_sequences.mjs',exit_code:0},
  ],
  scope:'Code and compact CAD inputs/results are versioned with this manifest. The scene is durably archived by the index. Binary is rebuilt from the recorded code overlay and lockfile. Successful sampled planning equivalence does not establish faster runtime walking, multi-step joint-force optimization, or a physical speed ceiling.'};
fs.writeFileSync(prefix+'contact-sequence-evidence.json',JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({code_files:code.length,artifacts:files.length,binary_sha256:manifest.binary.sha256}));
