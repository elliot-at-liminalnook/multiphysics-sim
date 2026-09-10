import fs from 'node:fs';
const d='examples/full-robot/fast-wasd',read=p=>JSON.parse(fs.readFileSync(`${d}/${p}.json`));
const sources=process.argv.slice(2);if(!sources.length)throw Error('source gait names required');
const cases=[];
for(const source of sources){
 const scene=read(`${source}.scene`),c=read(`${source}.config`),speed=scene.controller.parameters.nominal_speed_m_s,name=`human-${source}`;
 Object.assign(scene.controller.parameters,{command_lease:{period_s:.02,timeout_s:.25},acceleration_m_s2:.4,velocity_lead_s:scene.controller.parameters.velocity_lead_s??Array(12).fill(0)});
 scene.controller.inputs.push({name:'command.packet_sequence',kind:'Dimensionless',lower:0,upper:1000000000,initial:0});
 scene.controller.inputs.forEach(ch=>{if(ch.name==='command.yaw_rate'){ch.lower=-.06;ch.upper=.06;ch.initial=0}});
 scene.controller.sources={entry:'human-paired.rhai',files:{'human-paired.rhai':`
fn control(t, sensors, commands, state) {
 let p=parameters();
 if !state.contains("phase") {
  state.phase=0.0;state.rate=0.0;state.last_t=t;state.yaw_offsets=[0.0,0.0,0.0,0.0];
  state.lease=#{sequence:-1.0,age_s:p.command_lease.timeout_s};
 }
 state.lease=command_lease_update(state.lease.sequence,state.lease.age_s,sensors["command.packet_sequence"],p.command_lease);
 let dt=t-state.last_t;state.last_t=t;
 state.phase=(state.phase+dt*state.rate)%p.period_s;
 if state.phase<0.0 {state.phase+=p.period_s;}
 let u=state.phase%0.4;let all_stance=u<=p.swing_start_s||u>=p.swing_end_s;
 let requested=if state.lease.fresh {sensors["command.forward_speed"]/p.nominal_speed_m_s}else{0.0};
 if requested!=0.0 && requested*state.rate>=0.0 {
  let change=dt*p.acceleration_m_s2/p.nominal_speed_m_s;
  state.rate+=(requested-state.rate).max(-change).min(change);
 } else if all_stance {state.rate=0.0;}
 let yaw=if state.lease.fresh && requested!=0.0 {sensors["command.yaw_rate"]}else{0.0};
 for leg in 0..4 {
  let first_pair=leg==0||leg==2;
  let active_pair=(state.phase<0.4&&first_pair)||(state.phase>=0.4&&!first_pair);
  if active_pair&&u>p.swing_start_s&&u<p.swing_end_s {state.yaw_offsets[leg]*=0.65;}
  else {state.yaw_offsets[leg]=(state.yaw_offsets[leg]-p.yaw_jacobian_ratios[leg]*yaw*dt).max(-0.05).min(0.05);}
 }
 let cell=state.phase/0.02;let i=cell.floor().to_int();let blend=cell-i.to_float();
 for name in commands.keys() {
  let j=p.motor_indices[name];let target=p.samples[i][j]+blend*(p.samples[i+1][j]-p.samples[i][j]);
  let velocity=state.rate*(p.samples[i+1][j]-p.samples[i][j])/0.02;
  if j%3==0 {target+=state.yaw_offsets[j/3];}
  let joint=name.sub_string(0,name.len()-7);
  commands[name]=target+sensors["command.tracking_gain"]*(target-sensors[joint+".angle"])+p.velocity_lead_s[j]*velocity;
 }
 #{commands:commands,state:state}
}`}};
 scene.duration_s=20;c.steps=Math.round(20/c.step_s);
 const actions=Array.from({length:1000},(_,i)=>scene.controller.inputs.map(ch=>ch.name==='command.packet_sequence'?i+1:ch.name==='command.forward_speed'?(i<20||i>=800?0:i<500?speed:-speed):ch.name==='command.yaw_rate'?(i>=300&&i<500?.06:0):ch.initial));
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',actions]])fs.writeFileSync(`${d}/${name}.${suffix}.json`,JSON.stringify(value)+'\n');
 cases.push({name,source,speed_m_s:speed,step_s:c.step_s});
}
fs.writeFileSync(`${d}/human-trials.json`,JSON.stringify({cases,commands:[{start_s:0,forward_m_s:0,yaw_rad_s:0},{start_s:.4,forward:'full',yaw_rad_s:0},{start_s:6,forward:'full',yaw_rad_s:.06},{start_s:10,forward:'reverse',yaw_rad_s:0},{start_s:16,forward_m_s:0,yaw_rad_s:0}],scope:'20 s WASD sequence. Rust packet lease, phase-gated stop/reverse, bounded acceleration. Simulated command freshness uses controller time; hardware requires independent deadlines and real packet arrival sequence. Walking turns only.'},null,2)+'\n');
