import fs from 'node:fs';
const d='examples/full-robot/fast-wasd',read=p=>JSON.parse(fs.readFileSync(`${d}/${p}.json`));
const catalog=read('speed-trials');
for(const [source,speed] of [['hip0-65',.065],['landing-75',.075],['landing-100',.1]]){
 const name=`feedforward-${speed*1000}`,scene=read(`${source}.scene`),c=read(`${source}.config`);
 // For tau=K(q_command-q)-D*qdot, adding (D/K)*qdot_reference
 // cancels nominal velocity damping. Keep original modeled K,D and limits.
 const lead=c.motors.effective.components.map(m=>m.parameters.damping/m.parameters.stiffness);
 scene.controller.parameters.velocity_lead_s=lead;
 for(const key of Object.keys(scene.controller.sources.files))scene.controller.sources.files[key]=scene.controller.sources.files[key].replace('commands[name] = target +','let velocity = state.rate * (p.samples[i+1][j] - p.samples[i][j]) / 0.02;\n  commands[name] = p.velocity_lead_s[j] * velocity + target +');
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',read(`${source}.actions`)]])fs.writeFileSync(`${d}/${name}.${suffix}.json`,JSON.stringify(value)+'\n');
 catalog.definitions.push({name,method:'Nominal damping cancellation from effective CAD-derived actuator D/K',command_speed_m_s:speed,period_s:.8,source,velocity_lead_s:lead,scope:'Uses the existing provisional actuator model, no gain/torque authority increase. Saturation and contact remain physical.'});
}
fs.writeFileSync(`${d}/speed-trials.json`,JSON.stringify(catalog,null,2)+'\n');
