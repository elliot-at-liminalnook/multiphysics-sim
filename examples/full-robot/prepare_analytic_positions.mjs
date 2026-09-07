import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const directory='runs/full-robot/learning/analytic-positions';
const source='runs/full-robot/learning/closure-preparation';
const hash=b=>createHash('sha256').update(b).digest('hex');
const inputs={},outputs={};
async function read(p){const b=await readFile(p);inputs[p]=hash(b);return JSON.parse(b);}
await mkdir(directory,{recursive:true});
const parent=await read(source+'/manifest.json');
for(const name of ['base','refined']){
 const c=await read(`${source}/${name}.config.json`);
 c.embedding.analytic_mechanism_positions=true;
 const p=`${directory}/${name}.config.json`,b=JSON.stringify(c);
 await writeFile(p,b);outputs[p]=hash(b);
 if(name==='base') {
  const viewer={...c,profile_solver:false};
  const vp=`${directory}/viewer.config.json`,vb=JSON.stringify(viewer);
  await writeFile(vp,vb);outputs[vp]=hash(vb);
 }
}
inputs[parent.scene]=hash(await readFile(parent.scene));
inputs[import.meta.filename]=hash(await readFile(import.meta.filename));
await writeFile(directory+'/manifest.json',JSON.stringify({
 scene:parent.scene,inputs,outputs,
 comparisons:{base:source+'/base.execution.json',refined:source+'/refined.execution.json'},
 scope:'Opt-in certified analytic mechanism positions. Same rank/SVD diagnostics, numeric tangent and curvature, all original closure equations, actuator laws, contact and controller. Audit both branches independently. No safe travel limit, hardware calibration, realtime or training-model acceptance implied.'
},null,2));
console.log('Prepared analytic-position experiment');
