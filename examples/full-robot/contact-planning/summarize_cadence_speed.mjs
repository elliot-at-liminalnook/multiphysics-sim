import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
const dir='examples/full-robot/contact-planning/';
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const equal=(a,b,label)=>assert(isDeepStrictEqual(a,b),label);
const base=read('runs/contact-planning/diagonal-cadence211.native.json');
const source=read('runs/contact-planning/diagonal21-screen.scene.json');
const sourceConfig=read('runs/contact-planning/diagonal21-screen.config.json');
const sourceActions=read('runs/contact-planning/diagonal21-screen.actions.json');
const index=source.controller.inputs.findIndex(i=>i.name==='command.forward_speed');
const coordinates=read(dir+'diagonal21-reference.result.json').recipe.independent_coordinates;
const names=['211','230','250','280','230-fine'];
const results=[];
for(const name of names){
 const stem='diagonal-cadence'+name,spec=read(dir+stem+'-trial.json');
 for(const f of [...spec.sources,...spec.files])assert.equal(hash(f.path),f.sha256);
 const c=read(spec.prefix+'.native.json'),s=read(dir+stem+'.summary.json'),a=read(dir+stem+'-clearance.summary.json');
 assert(c.completed&&c.error===null&&c.frames.length===401&&c.frames.at(-1).time_s===8);
 assert.equal(c.recording.seed,0);
 const contract=structuredClone(c.contract);contract.actions[index]=base.contract.actions[index];
 equal(contract,base.contract,'common environment contract except declared speed bounds');
 const scene=read(spec.prefix+'.scene.json');scene.controller.inputs[index]=source.controller.inputs[index];
 equal(scene,source,'only commanded speed bounds change in scene');
 const config=read(spec.prefix+'.config.json');
 for(const k of ['step_s','steps','report_every'])config[k]=sourceConfig[k];
 equal(config,sourceConfig,'only declared timestep fields change in config');
 const nativeScene=structuredClone(c.recording.scene);
 nativeScene.controller.inputs[index]=base.recording.scene.controller.inputs[index];
 equal(nativeScene,base.recording.scene,'same normalized runtime scene');
 const nativeConfig=structuredClone(c.recording.config);
 for(const k of ['step_s','steps','report_every'])nativeConfig[k]=base.recording.config[k];
 equal(nativeConfig,base.recording.config,'same normalized runtime config');
 const actions=read(spec.prefix+'.actions.json');
 for(const row of actions){assert(row[index]===0||Math.abs(row[index])===spec.command_speed_m_s);
  row[index]=Math.sign(row[index])*source.controller.parameters.nominal_speed_m_s;}
 equal(actions,sourceActions,'same command times and other controls');
 assert.equal(s.capture_sha256,hash(spec.prefix+'.native.json'));
 assert.equal(s.command_speed_m_s,spec.command_speed_m_s);
 assert.equal(a.maximum_command_error_rad,0);
 const motorPeaks=c.frames[0].motor_readings.map((_,i)=>({
  coordinate:coordinates[i],
  maximum_recorded_shaft_speed_rad_s:Math.max(...c.frames.map(f=>Math.abs(f.motor_readings[i].gear_speed_rad_s))),
  maximum_recorded_shaft_torque_nm:Math.max(...c.frames.map(f=>Math.abs(f.motor_readings[i].shaft_torque_nm)))
 }));
 results.push({name,command_speed_m_s:s.command_speed_m_s,step_s:s.physics_step_s,
  speeds_m_s:s.segments.map(x=>x.speed_along_heading_m_s),maximum_slip_ratio:s.maximum_slip_ratio,
  passed_control_checks:s.passed_control_checks,passed_contact_quality:s.passed_contact_quality,
  planned_lifts:a.planned_foot_clearances,passed_lifts:a.passed_foot_clearances,
  geometry:a.inter_link_geometry_audit,motor_peaks:motorPeaks});
}
const coarse=read('runs/contact-planning/diagonal-cadence230.native.json');
const fine=read('runs/contact-planning/diagonal-cadence230-fine.native.json');
let maximumPathDifference=0;
for(let i=0;i<coarse.frames.length;i++){
 const a=coarse.frames[i],b=fine.frames[i];assert.equal(a.time_s,b.time_s);
 const body=f=>f.poses.find(p=>p.name.includes('Chassis')).position_m;
 maximumPathDifference=Math.max(maximumPathDifference,Math.hypot(...body(a).map((v,j)=>v-body(b)[j])));
}
const capability=read(dir+'diagonal21-cadence-capability.result.json');
console.log(JSON.stringify({results,timestep_comparison:{coarse_s:.000625,fine_s:.0003125,
 maximum_chassis_position_difference_m:maximumPathDifference},
 reference_no_load_rate_budget_m_s:capability.reference_cycle_rate_budget_speed_m_s,
 reference_coordinates:capability.reference_cycle_coordinates,
 scope:'Matched eight-second runtime speed screens with unchanged physical model and gates. Shaft extrema are recorded sample extrema, not continuous peaks. Reference rate budget is conditional, not a physical speed ceiling. No candidate passes all slip/lift/geometry checks; faster sliding or overlap is not a qualified gait. No sustained, steering, browser or sim-to-real promotion.'},null,2));
