// Experiment configuration only. All dynamics execute in the Rust library.
import fs from 'node:fs';
const root = new URL('.', import.meta.url);
const motor = JSON.parse(fs.readFileSync(new URL('cad-motor.json',root)));
const speed=Math.PI/3/0.19, voltage=11.1, current=3, idle=0.1, ratio=200;
const resistance=voltage/current, ke=(voltage-resistance*idle)/(ratio*speed);
const reciprocal={torque_constant:ke,back_emf_constant:ke,ratio,
 efficiency:motor.gearbox.max_output_torque/(ratio*ke*current),gear_friction:0,
 gear_inertia:0.00002,derating:0};
const base={voltage_v:voltage,duration_s:1,step_s:0.00025,sample_s:0.001,
 load_inertia_kg_m2:0.001,load_torque_nm:0,locked:false,thermal:false,
 ambient_c:25,servo:false,target_rad:Math.PI/3,frequency_hz:0,
 motor_overrides:reciprocal,firmware_overrides:{},thermal_resistance_scale:1,encoder_parameters:{counts:4096,period:0.001,seed:2301}};
const cases=[];const add=(name,extra={})=>cases.push({...base,name,...extra});
for(const v of [9,11.1,12.6])for(const fraction of [0,0.25,0.5,0.75,0.9])
 add(`curve_v${v}_load${fraction}`,{voltage_v:v,load_torque_nm:motor.gearbox.max_output_torque*fraction*v/voltage});
for(const v of [9,11.1,12.6])add(`stall_v${v}`,{voltage_v:v,locked:true});
add('cad_free',{motor_overrides:{}});add('cad_stall',{motor_overrides:{},locked:true});
for(const j of [0.0001,0.001,0.01])add(`step_j${j}`,{servo:true,load_inertia_kg_m2:j});
for(const load of [0.5,1,2])add(`hold_load${load}`,{servo:true,load_torque_nm:load,duration_s:2});
for(const f of [0.5,1,2,4,8])add(`sine_f${f}`,{servo:true,frequency_hz:f,target_rad:0.25,duration_s:Math.max(2,4/f)});
for(const latency of [0.001,0.005,0.02])add(`latency_${latency}`,{servo:true,firmware_overrides:{latency}});
for(const backlash of [0.003490658503988659,0.017453292519943295])add(`backlash_${backlash}`,{servo:true,frequency_hz:2,target_rad:0.25,duration_s:2,motor_overrides:{...reciprocal,backlash}});
for(const k of [20,200])add(`stiffness_${k}`,{servo:true,motor_overrides:{...reciprocal,gear_stiffness:k,gear_damping:0.002*k}});
for(const scale of [0.5,1,2])add(`thermal_stall_r${scale}`,{locked:true,thermal:true,duration_s:60,step_s:0.005,sample_s:0.05,thermal_resistance_scale:scale});
add('thermal_free',{thermal:true,duration_s:60,step_s:0.005,sample_s:0.05});
add('thermal_halfload',{thermal:true,load_torque_nm:motor.gearbox.max_output_torque/2,duration_s:60,step_s:0.005,sample_s:0.05});
for(const dt of [0.0005,0.000125]){
 add(`step_dt${dt}`,{servo:true,step_s:dt});
 add(`sine_dt${dt}`,{servo:true,frequency_hz:4,target_rad:0.25,duration_s:2,step_s:dt});
}
add('thermal_stall_fine',{locked:true,thermal:true,duration_s:60,step_s:0.0025,sample_s:0.05});
for(const period of [0.005,0.02])add('encoder_period'+period,{servo:true,frequency_hz:2,target_rad:0.25,duration_s:2,encoder_parameters:{counts:4096,period,seed:2301}});
for(const dt of [0.0000625,0.00003125])add('step_dt'+dt,{servo:true,step_s:dt});
for(const dt of [0.00001,0.000005])add('startup_dt'+dt,{servo:true,step_s:dt,sample_s:0.00001,duration_s:0.02});
// Thermal tests use an ideal unsampled observation: sensor ticks must not
// silently subdivide both timesteps in the integration sensitivity check.
for(const c of cases)if(c.thermal)c.encoder_parameters={counts:0,period:0,seed:2301};
const provenance={status:'Uncalibrated simulation study, no hardware measurements',date:'2026-09-11',
 cad_source:'examples/full-robot/trusted-baseline/fastest573.input.json :: runtime.scene.robot.motors[0]',
 cad_artifact:'examples/full-robot/baseline/robot.rcad',cad_sha256:'2fc4523f1fefa5ff3530f1d6814ec4f1a9e5c345b4ce8a924686cd654e3c0589',
 sources:['https://www.hiwonder.com/products/hx-30hm','https://wiki.hiwonder.com/projects/NexArm/en/esp32-version/docs/2_ESP32_Development_Basics.html'],
 published:{rated_voltage_v:11.1,voltage_range_v:[9,12.6],stall_torque_nm:2.941995,speed_rad_s:speed,stall_current_a:3,no_load_current_a:0.1,mass_kg:0.052,encoder_bits:12,accuracy_deg:0.3,default_baud:1000000},
 derivations:{resistance:'V/Istall, a total equivalent resistance; copper/driver split unknown',back_emf:'(V-R*I0)/(N*omega0)',torque_constant:'Kt=Ke in SI for reciprocal energy conversion',efficiency:'tau_stall/(N*Kt*Istall)',ratio:'200:1 hypothetical decomposition, not identified; many N/K/J combinations fit same endpoints',thermal:'CAD estimated copper mass 12% of 52g, Cw=mass_copper*385; remainder*900 for case; mounting held at ambient',gear_inertia:'2e-5 kg m2 illustrative output inertia, not measured',gear_friction:'zero additional Coulomb loss avoids double counting losses already fitted with I0/efficiency',derating:'0 preserves Kt=Ke; temperature changes winding R, magnet temperature coefficient unknown'},
 limitations:['Open-voltage curves use ideal terminal voltage with no firmware protection/current limit; stall at 12.6V is an extrapolated boundary test.', 'Position/sine tests use shared sampled PD/quantization/latency and averaged H-bridge; proprietary trapezoidal command shaping, speed-loop modes, UART parsing and protection thresholds not identified.', 'Thermal protection disabled to expose hypothetical plant heating; 110C is a CAD provisional marker, never a measured shutdown threshold.', 'All mechanical/driver heat in motor thermal port is lumped; H-bridge loss is not coupled into case.', 'Encoder quantization is modeled; accuracy, magnetic field geometry, noise, eccentricity, temperature drift and bus delay are not inferred from bit count.', 'Cold curves hold winding at 25C. Unknowns are scenario sweeps, not confidence intervals.'],
 reciprocal_parameters:reciprocal};
fs.writeFileSync(new URL('plan.json',root),JSON.stringify({motor,cases,provenance},null,2)+'\n');
console.log(`${cases.length} cases; reciprocal efficiency ${reciprocal.efficiency}`);
