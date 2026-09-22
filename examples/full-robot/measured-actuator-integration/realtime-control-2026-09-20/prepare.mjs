import fs from'node:fs';import path from'node:path';import{createHash}from'node:crypto';
const here=import.meta.dirname,base=path.resolve(here,'../browser-control-400hz');
const read=p=>JSON.parse(fs.readFileSync(p));const scene=read(base+'/scene.json'),config=read(base+'/config.json'),task=read(base+'/task.json');
config.steps=Math.round(3/config.step_s);
const inputs=read(base+'/verification/reference.browser.json').inputs;
const actions=Array.from({length:150},(_,i)=>inputs.map(c=>c.name==='command.forward_speed'?.1:c.name==='command.packet_sequence'?i+1:c.initial));
for(const [name,data]of Object.entries({scene,config,task,actions}))fs.writeFileSync(here+'/'+name+'.json',JSON.stringify(data));
const hash=p=>createHash('sha256').update(fs.readFileSync(p)).digest('hex');
fs.writeFileSync(here+'/protocol.json',JSON.stringify({version:1,simulation_only:true,seed:0,simulated_seconds:3,motor_hz:400,policy_hz:50,physics_hz:6400,command:'forward 0.1 m/s with incrementing heartbeat each policy tick',source:Object.fromEntries(['scene','config','task'].map(k=>[k,{path:path.relative(path.resolve(here,'../../../..'),base+'/'+k+'.json'),sha256:hash(base+'/'+k+'.json')}])),gates:{candidate_minimum_speedup:1.2,realtime_sim_per_wall:1,policy_step_p95_s:.02,motor_angle_rad:.0017453292519943296,link_position_m:.001,current_a:.1},scope:'Identical native and WASM inputs, seed, model and clocks. Profile timers are diagnostic, excluded from performance acceptance. No promotion solely from short forward case. Structural optimizations must preserve physical laws and original solver acceptance checks.'},null,2));
// Explicit solver recipes and one fixed forward/turn/reverse/stop schedule.
for(const [name,extra]of [['warm',{reuse_auxiliary_solve:true}],['warm-probes',{reuse_auxiliary_solve:true,linearized_jacobian_probes:true,reuse_exact_probe_base:true}]]){
 fs.mkdirSync(here+'/'+name,{recursive:true});for(const file of ['scene','task','actions'])fs.copyFileSync(`${here}/${file}.json`,`${here}/${name}/${file}.json`);
 fs.writeFileSync(`${here}/${name}/config.json`,JSON.stringify({...config,implicit:{...config.implicit,...extra}}));
}
const forward=inputs.findIndex(c=>c.name==='command.forward_speed'),yaw=inputs.findIndex(c=>c.name==='command.yaw_rate');
const sequence=actions.map((old,i)=>{const a=[...old];a[forward]=i<50?.1:i<75?.05:i<100?-.05:0;a[yaw]=i>=50&&i<75?.15:0;return a;});
for(const name of ['baseline','warm','warm-probes']){
 const output=here+'/sequence-'+name;fs.mkdirSync(output,{recursive:true});for(const file of ['scene','task'])fs.copyFileSync(`${here}/${file}.json`,`${output}/${file}.json`);
 fs.copyFileSync(here+(name==='baseline'?'':'/'+name)+'/config.json',output+'/config.json');fs.writeFileSync(output+'/actions.json',JSON.stringify(sequence));
}
const protocol=read(here+'/protocol.json');protocol.gates.contact_force_n=1;protocol.gates.require_equal_contact_identities=true;fs.writeFileSync(here+'/protocol.json',JSON.stringify(protocol,null,2));
