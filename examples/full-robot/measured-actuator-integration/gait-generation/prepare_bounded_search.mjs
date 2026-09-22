// Explicit controller/search configuration only. All control and physics are Rust/Rhai.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const [source,out,seconds='3']=process.argv.slice(2);
if(!source||!out)throw Error('prepare_bounded_search.mjs CAD_BOUND_SPEC NEW_DIRECTORY [seconds]');
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const spec=JSON.parse(fs.readFileSync(source));
const horizon=Number(seconds),dt=spec.scene.period_s;
assert(horizon>=2&&horizon<=30&&Math.abs(horizon/dt-Math.round(horizon/dt))<1e-9);
const program=spec.scene.controller,p=program.parameters;
assert.equal(dt,0.02);
let script=program.sources.files[program.sources.entry];
const replace=(a,b)=>{assert.equal(script.split(a).length,2,`Unique source anchor required: ${a}`);script=script.replace(a,b);};
p.reference_centers=p.trajectory.keyframes[0].values.map((_,i)=>p.trajectory.keyframes.reduce((s,r)=>s+r.values[i],0)/p.trajectory.keyframes.length);
p.motion_amplitude=0.7;p.velocity_lead_scale=0.;
p.reference_governor={period_s:dt,maximum_speed_rad_s:80*Math.PI/180,maximum_acceleration_rad_s2:400*Math.PI/180,response_rate_per_s:10};
replace('let j=p.motor_indices[name];let target=reference.values[j];','let j=p.motor_indices[name];let target=p.reference_centers[j]+p.motion_amplitude*(reference.values[j]-p.reference_centers[j]);');
replace('+p.velocity_lead_s[j]*state.rate*reference.rates[j]','+p.velocity_lead_scale*p.motion_amplitude*p.velocity_lead_s[j]*state.rate*reference.rates[j]');
replace('        response.commands[name] = applied;',`        if !response.state.contains("governors") {response.state.governors=#{};}
        if !response.state.governors.contains(name) {
            let joint=name.sub_string(0,name.len()-7);
            response.state.governors[name]=#{angle_rad:sensors[joint+".angle"],velocity_rad_s:0.0};
        }
        let old=response.state.governors[name];
        let next=reference_governor_update(old.angle_rad,old.velocity_rad_s,applied,p.reference_governor);
        if next.angle_rad<bounds[0] || next.angle_rad>bounds[1] {throw "bounded reference crossed authored angle limit";}
        response.state.governors[name]=next;
        response.commands[name] = next.angle_rad;`);
program.sources={entry:'bounded-gait.rhai',files:{'bounded-gait.rhai':script}};
const constant=value=>({source:'constant',value});const parameter=name=>({source:'parameter',name});
spec.parameterization={version:1,space:{parameters:[
    {name:'pace_scale',kind:'Dimensionless',bounds:[0.15,1]},
    {name:'motion_amplitude',kind:'Dimensionless',bounds:[0.45,1.1]},
    {name:'tracking_gain',kind:'Dimensionless',bounds:[-0.5,0.2]},
    {name:'velocity_lead_scale',kind:'Dimensionless',bounds:[0,0.2]},
  ]},trajectories:[],commands:spec.parameterization.commands,
  scalars:['motion_amplitude','velocity_lead_scale'].map(name=>({pointer:`/${name}`,kind:'Dimensionless',reference:p[name],value:parameter(name)}))};
spec.baseline={pace_scale:0.35,motion_amplitude:0.7,tracking_gain:0,velocity_lead_scale:0};
spec.config.steps=Math.round(horizon/spec.config.step_s);
spec.scene.duration_s=horizon;
spec.source_actions=spec.source_actions.slice(0,Math.round(horizon/dt));
assert.equal(spec.source_actions.length,Math.round(horizon/dt));
// Applied targets are already recorded as frame.servo_targets_rad. The separate
// ReferencePosition observation denotes the authored reference, not policy output.
fs.mkdirSync(out);const write=(name,x)=>fs.writeFileSync(`${out}/${name}`,JSON.stringify(x)+'\n',{flag:'wx'});
write('pilot.spec.json',spec);write('pilot.settings.json',{seed:2301,initial_design:5,acquisition_starts:4,maximum_training_rows:32});
write('preparation.json',{source,source_sha256:sha(source),script_sha256:sha(new URL(import.meta.url)),horizon_s:horizon,
  scope:'Bounded references are active inside every full-robot trial. Authored speed/acceleration envelope, not measured hardware limits. Gains/delay are inherited from the separate CAD scenario; original geometry and mechanics are preserved.',
  controller_modifications:['Shared Rust reference governor called by Rhai','Amplitude about the original trajectory mean','Explicit velocity-lead scaling','Pace and tracking-feedback search'],
  reference_governor:p.reference_governor,baseline:spec.baseline});
fs.writeFileSync(`${out}/bounded-gait.rhai`,script,{flag:'wx'});
console.log(JSON.stringify({out,horizon_s:horizon,baseline:spec.baseline}));
