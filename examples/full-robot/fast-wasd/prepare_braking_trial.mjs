import fs from 'node:fs';const d='examples/full-robot/fast-wasd',read=p=>JSON.parse(fs.readFileSync(`${d}/${p}.json`));
const source='human-feedforward-65',name='human-braked-65',scene=read(`${source}.scene`),c=read(`${source}.config`);
Object.assign(scene.controller.parameters,{deceleration_m_s2:.8,reversal_settle_s:.08});
let code=scene.controller.sources.files['human-paired.rhai'];
code=code.replace('state.phase=0.0;','state.braking=false;state.hold_until=0.0;state.phase=0.0;');
code=code.replace('state.phase=(state.phase+dt*state.rate)%p.period_s;',`
 let advance=dt*state.rate;
 if state.braking {
  let old_u=state.phase%0.4;
  let remaining=if state.rate>0.0 {if old_u<=p.swing_start_s {p.swing_start_s-old_u}else{0.4+p.swing_start_s-old_u}}
                else {if old_u>=p.swing_end_s {old_u-p.swing_end_s}else{old_u+0.4-p.swing_end_s}};
  if advance.abs()>=remaining {advance=advance.sign()*(remaining-0.000000001).max(0.0);state.rate=0.0;}
 }
 state.phase=(state.phase+advance)%p.period_s;`);
code=code.replace('if requested!=0.0 && requested*state.rate>=0.0 {',`
 if state.braking || (all_stance && state.rate!=0.0 && requested*state.rate<=0.0) {
  state.braking=true;
  let change=dt*p.deceleration_m_s2/p.nominal_speed_m_s;
  state.rate=state.rate.sign()*(state.rate.abs()-change).max(0.0);
  if state.rate==0.0 {state.braking=false;state.hold_until=t+p.reversal_settle_s;}
 } else if t<state.hold_until {state.rate=0.0;}
 else if requested!=0.0 && requested*state.rate>=0.0 {`);
scene.controller.sources.files['human-paired.rhai']=code;
for(const [suffix,value] of [['scene',scene],['config',c],['actions',read(`${source}.actions`)]])fs.writeFileSync(`${d}/${name}.${suffix}.json`,JSON.stringify(value)+'\n');
const trials=read('human-trials');trials.cases.push({name,source,speed_m_s:.065,step_s:c.step_s,scope:'Brake within the all-stance interval, then settle 80 ms before reversal. Cap phase advance at the next lift boundary during braking.'});fs.writeFileSync(`${d}/human-trials.json`,JSON.stringify(trials,null,2)+'\n');
