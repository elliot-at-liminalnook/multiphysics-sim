import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual as equal} from 'node:util';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/drive-backlash-definition',browser='runs/interactive/drive-backlash-definition',artifacts={};
async function bytes(p){const b=await readFile(p);artifacts[p]=createHash('sha256').update(b).digest('hex');return b;}
async function read(p){return JSON.parse(await bytes(p));}
const manifest=await read(root+'/manifest.json');
for(const [p,h]of Object.entries({...manifest.inputs,...manifest.outputs})){await bytes(p);assert.equal(artifacts[p],h,p);}
const scene=await read(manifest.scene),old=await read(manifest.source_scene);
const restored=structuredClone(scene);restored.robot.version=old.robot.version;
for(const o of manifest.overrides){const j=restored.robot.joints.find(j=>j.name===o.joint);assert(equal(j.physics.drive_backlash,o.drive_backlash));delete j.physics.drive_backlash;}
assert(equal(restored,old),'unrecorded physical model change');
const sections=['frames','terminal_frame','hybrid_steps','hybrid_solves','contact_steps','contact_impulses'];
{
 const legacy=await read('runs/full-robot/learning/analytic-positions/base.execution.json'),compat=await read(root+'/compatibility.execution.json');
 assert(compat.completed&&compat.error===null);
 for(const k of sections)assert(equal(compat[k],legacy[k]),`migration changed ${k}`);
}
const cases=[];
for(const c of manifest.cases){
 const d=await read(root+'/'+c.name+'.execution.json'),l=await read(root+'/'+c.name+'.lift.json');
 assert(d.completed&&d.error===null&&d.frames.length===281);assert(l.report.passed);
 const closure={};
 for(const f of d.frames)for(const row of f.original_rows){
  const max=closure[row.unit]??={position:0,velocity:0,acceleration:0};
  const scale=row.unit==='m'?d.embedding.length_scale_m:row.unit==='rad'?d.embedding.angle_scale_rad:1;
  for(const k of Object.keys(max)){assert(Number.isFinite(row[k]));max[k]=Math.max(max[k],Math.abs(row[k]));assert(Math.abs(row[k])/scale<=d.embedding.scaled_closure_tolerance,`${c.name}: ${row.name} ${k}`);}
 }
 cases.push({name:c.name,step_s:d.step_s,simulated_s:d.simulated_s,development_wall_s:d.stepping_wall_s,lift:l.report,closure,
  accepted_segments:d.hybrid_steps.reduce((s,x)=>s+x.accepted_segments,0),rejected_trials:d.hybrid_steps.reduce((s,x)=>s+x.rejected_trials,0)});
}
const limits=(await read('runs/full-robot/learning/fast-motor/manifest.json')).preliminary_screen;
const candidate=await read(root+'/1000us.execution.json'),fine=await read(root+'/62p5us.execution.json'),comparison=await read(root+'/1000us-fine.comparison.json');
let motorError=0;
for(let i=0;i<candidate.frames.length;i++){
 assert.equal(candidate.frames[i].time_s,fine.frames[i].time_s);
 for(const j of candidate.independent_joint_indices)motorError=Math.max(motorError,Math.abs(candidate.frames[i].joint_positions[j]-fine.frames[i].joint_positions[j]));
}
const foot=Math.max(...comparison.markers.map(m=>m.maximum_error_m)),rms=Math.max(...comparison.markers.map(m=>m.rms_error_m));
const impulses=comparison.contact_impulse_comparison.total.map(i=>({...i,limit_ns:Math.max(limits.absolute_impulse_floor_ns,limits.maximum_relative_per_foot_impulse_difference*Math.hypot(...i.reference_ns))}));
const checks={foot:foot<=limits.maximum_foot_difference_m,rms:rms<=limits.rms_foot_difference_m,motor_angle:motorError<=limits.maximum_motor_angle_difference_rad,
 impulses:impulses.every(i=>i.difference_norm_ns<=i.limit_ns),supported_lift:cases.find(c=>c.name==='1000us').lift.passed};
assert(Object.values(checks).every(Boolean),'candidate failed existing provisional task screen');
const benchmark=await read(root+'/benchmark/manifest.json');assert.equal(benchmark.runs.length,4);
for(const [p,h]of Object.entries(benchmark.inputs)){await bytes(p);assert.equal(artifacts[p],h,p);}
for(const run of benchmark.runs){await bytes(run.output);assert.equal(artifacts[run.output],run.sha256);}
const robotBrowser=await read(browser+'/robot-browser.json'),fixtureBrowser=await read(browser+'/fixture-browser.json'),ui=await read(browser+'/viewer-report.json');
assert(robotBrowser.passed&&robotBrowser.compared_frames===281&&robotBrowser.unknown_drive_rejected_without_mutation);
assert(fixtureBrowser.passed&&fixtureBrowser.unknown_drive_rejected_without_mutation&&ui.passed);
assert(ui.checks.some(c=>c.startsWith('robot-drive-definition ')));
for(const p of ['cad/robocad/physical.py','cad/robocad/commands.py','cad/robocad/client.py','cad/robocad/ui/widgets.py','cad/tests/test_drive_backlash.py','cad/tests/test_physical.py','cad/tests/test_ui.py',
 'cad/PHYSICAL_MODEL.md','cad/USER_GUIDE.md','crates/sim-domain-robot/src/model.rs','crates/sim-domain-robot/src/motor.rs','crates/sim-runtime/src/physical.rs','crates/sim-runtime/src/embedded.rs','crates/sim-runtime/tests/drive_backlash.rs',
 'examples/interactive/pendulum.drive-backlash.scene.json','web/tests/embedded.mjs','web/tests/viewer.mjs','web/viewer/presets.json','.github/workflows/browser.yml',
 browser+'/native-runner',browser+'/viewer/build-manifest.json',browser+'/viewer/sim_web_bg.wasm','examples/full-robot/drive-backlash-validation.md',import.meta.filename])await bytes(p);
const result={scope:'Explicit v4 drive definition and provisional 1 ms robot experiment. All drive-connection gaps are estimated as zero; motor gearbox calibration, electrical transient accuracy, hardware transfer, walking and learning remain unaccepted.',
 training_model_accepted:false,realtime_accepted:false,legacy_migration_exact:true,cases,
 provisional_screen:{limits,checks,reference_step_s:fine.step_s,maximum_foot_difference_m:foot,rms_foot_difference_m:rms,maximum_motor_angle_difference_rad:motorError,impulses,
 maximum_sampled_differences:comparison.maximum_sampled_differences,contact_pair_mismatch_samples:comparison.contact_pair_mismatch_samples},
 benchmark,robot_browser:robotBrowser,fixture_browser:fixtureBrowser,viewer:ui,artifacts};
await writeFile('examples/full-robot/drive-backlash-status.json',JSON.stringify(result,null,2));
console.log(JSON.stringify({legacy_migration_exact:true,cases:cases.length,checks,foot_mm:1000*foot,motor_rad:motorError,benchmark:benchmark.result,viewer_checks:ui.checks.length,artifacts:Object.keys(artifacts).length}));
