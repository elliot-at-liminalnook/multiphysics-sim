// Immutable isolated viewer from declared scene/config/task profiles.
import fs from 'node:fs';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import assert from 'node:assert/strict';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const specPath=process.argv[2];assert(specPath,'profile spec required');
const spec=read(specPath),out=spec.output;
assert(/^runs\/speed-ceiling\/viewer-[a-z0-9-]+$/.test(out),'isolated named viewer output required');
assert(!fs.existsSync(out),'refusing overwrite viewer');
assert(/^[a-z0-9-]+$/.test(spec.id)&&spec.profiles.length,'named nonempty profile spec required');
fs.mkdirSync(`${out}/data`,{recursive:true});
let run=spawnSync('tar',['-xzf','examples/full-robot/gait-exploration/browser-template.tar.gz','-C',out]);assert.equal(run.status,0);
for(const file of ['viewer.js','motion-commands.mjs'])fs.copyFileSync(`web/viewer/${file}`,`${out}/${file}`);
for(const file of ['worker.js','worker-message.mjs'])fs.copyFileSync(`web/${file}`,`${out}/${file}`);
run=spawnSync('/Users/elliot/.cargo/bin/wasm-bindgen',[spec.wasm,'--target','web','--out-name','sim_web','--out-dir',out],{stdio:'inherit'});assert.equal(run.status,0);
const presets=[],inputs=new Set([specPath,'Cargo.lock','crates/sim-domain-control/src/trajectory.rs','crates/sim-script/src/lib.rs','web/viewer/viewer.js','web/viewer/motion-commands.mjs','web/worker.js','web/worker-message.mjs','examples/full-robot/speed-ceiling/package_browser_profiles.mjs']);
for(const profile of spec.profiles) {
  assert(/^[a-z0-9-]+$/.test(profile.id)&&!presets.some(p=>p.id===profile.id),'unique profile id required');
  const scene=read(profile.scene),config=read(profile.config),task=read(profile.task);
  for(const path of [profile.scene,profile.config,profile.task])inputs.add(path);
  const path=`data/${profile.id}.json`;
  fs.writeFileSync(`${out}/${path}`,JSON.stringify({scene,config,task,seed:0})+'\n');
  presets.push({id:profile.id,label:profile.label,mode:'embedded',path,task:profile.task,
    motion_commands:['command.forward_speed','command.lateral_speed','command.yaw_rate'],motion_heartbeat:'command.packet_sequence',
    asset_sha256:hash(`${out}/${path}`),description:'Hold W/S for forward/reverse; combine A/D to steer. Release requests braking after foot transfer. Walking turns; no pivot turn.',
    readiness:profile.readiness,evidence:profile.evidence});
}
fs.writeFileSync(`${out}/catalog.json`,JSON.stringify({presets},null,2)+'\n');
fs.writeFileSync(`${out}/leaderboard.json`,JSON.stringify({version:1,entries:[],scope:'Isolated provisional experiment bundle; no ranked controller claim.'})+'\n');
const manifest={version:1,id:spec.id,wasm:{artifact_sha256:hash(spec.wasm),browser_module_sha256:hash(`${out}/sim_web_bg.wasm`)},
  inputs:Object.fromEntries([...inputs].map(path=>[path,hash(path)])),presets,
  scope:'Shared Rust/WASM runtime and explicit numerical profiles. Native physical qualification, cross-host parity and rendered browser performance are separate requirements. Building this bundle does not establish their acceptance.'};
fs.writeFileSync(`${out}/build-manifest.json`,JSON.stringify(manifest,null,2)+'\n');
fs.writeFileSync(`${d}/${spec.id}-browser-manifest.json`,JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
console.log(out);
