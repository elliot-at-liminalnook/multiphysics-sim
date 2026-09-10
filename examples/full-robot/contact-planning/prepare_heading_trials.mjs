// Change travel relative to the unchanged CAD chassis; do not rotate the robot/world.
import fs from 'node:fs';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning',read=p=>JSON.parse(fs.readFileSync(p));
const sources=[d+'/bidirectional-restoration.recipe.json',d+'/controller-reference.recipe.json'];
const base=read(sources[0]),ref=read(sources[1]),distance=Math.hypot(...ref.motion.displacement_world_m);
const rows=[];
for(const [name,degrees] of [['heading0',0],['heading-plus45',45],['heading-minus45',-45]]) {
 const p=structuredClone(base),angle=degrees*Math.PI/180,direction=[Math.cos(angle),Math.sin(angle),0];
 p.robot.direction_world=direction;
 p.motion.displacement_world_m=direction.map(v=>v*distance);
 p.variables.find(v=>v.decision.kind==='displacement').decision={kind:'displacement_along_direction'};
 // Foot 0 fixes the arbitrary cycle origin; all other foot phases may vary.
 if(!p.variables.some(v=>v.decision.kind==='foot_phase'&&v.decision.foot===2))
  p.variables.push({decision:{kind:'foot_phase',foot:2},bound:{lower:0,upper:.9999}});
 p.robot.uniform_samples=128;delete p.adaptive;p.search.maximum_evaluations=1;
 const path=d+'/'+name+'-audit.recipe.json';fs.writeFileSync(path,JSON.stringify(p,null,2)+'\n',{flag:'wx'});
 rows.push({name,degrees,recipe:path,direction_world:direction,initial_speed_m_s:distance/p.motion.period_s});
}
fs.writeFileSync(d+'/heading-trials.json',JSON.stringify({rows,sources:sources.map(path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')})),
 scope:'Matched initial speed, physical CAD pose, foot centers, world, actuators and tolerances. Only the travel vector changes relative to the chassis. Scalar travel distance preserves the requested heading during optimization. Initial body oscillations and contact timings are warm starts, not optimized independently for these headings. Both forward and reverse clocks are checked. This first audit is not a maximum-speed comparison.'},null,2)+'\n',{flag:'wx'});
