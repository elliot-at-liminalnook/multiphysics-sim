import fs from 'node:fs';
const d='examples/full-robot/gait-exploration',read=p=>JSON.parse(fs.readFileSync(`${d}/${p}`));
let catalog=read('candidates.json');
const append=(def,p)=>{if(catalog.definitions.some(x=>x.name===def.name))throw Error('duplicate candidate');def.planning=`${d}/${def.name}.planning.json`;fs.writeFileSync(def.planning,JSON.stringify(p)+'\n');catalog.definitions.push(def)};
for(let scale of [2,4]){let p=read('opposite-pair.planning.json'),base=catalog.definitions.find(x=>x.name==='opposite-pair'),def=structuredClone(base);def.name=`opposite-pair-${25*scale}mm`;def.stride_m*=scale;def.predicted_schedule_speed_mm_s*=scale;for(let k of p.base_displacements_world_m.keyframes)k.values[0]*=scale;for(let k of p.displacements_world_m.keyframes)for(let i=0;i<4;i++)k.values[3*i]*=scale;append(def,p)}
// Flight timing hypothesis: finish lift before most horizontal swing, then
// finish horizontal travel before lowering. Same period, stride, body path.
{let p=read('opposite-pair.planning.json'),base=catalog.definitions.find(x=>x.name==='opposite-pair'),def=structuredClone(base);def.name='opposite-pair-clear-flight';let smooth=u=>u*u*u*(10+u*(-15+6*u));
 for(let k of p.displacements_world_m.keyframes){let e=k.time_s-.4;if(e<0||e>=4.8)continue;let transfer=Math.floor((e+1e-9)/.4),u=(e-transfer*.4-.08)/.24,su=Math.max(0,Math.min(1,u));let group=def.groups[transfer%2];for(let i of group){let completed=Math.floor(transfer/2);let horiz=Math.max(0,Math.min(1,(su-.2)/.6));k.values[3*i]=def.stride_m*(completed+smooth(horiz));k.values[3*i+2]=def.lift_m*(su<1/3?smooth(su*3):su>2/3?smooth((1-su)*3):1)}}append(def,p)}
for(let baseName of ['long-stride-wave','continuous-wave']){let p=read(`${baseName}.planning.json`),base=catalog.definitions.find(x=>x.name===baseName),def=structuredClone(base);def.name=baseName+'-compact-support';def.stride_m=.06;def.transfer_s*=.75;def.swing_s*=.75;def.duration_s=.4+def.cycles*4*def.transfer_s+.8;
 // Resample the original piecewise-linear geometric recipe in normalized time.
 const sample=(ks,t)=>{let j=ks.findIndex(k=>k.time_s>=t-1e-10);if(j<=0)return [...ks[0].values];let a=ks[j-1],b=ks[j],u=(t-a.time_s)/(b.time_s-a.time_s);return a.values.map((x,i)=>x+u*(b.values[i]-x))};
 const oldBody=p.base_displacements_world_m.keyframes,oldFeet=p.displacements_world_m.keyframes;let body=[],feet=[];
 for(let tick=0;tick<=Math.round(def.duration_s/.02);tick++){let t=Number((tick*.02).toFixed(8)),e=t-.4,old=.4+Math.min(Math.max(e,0),def.cycles*4*def.transfer_s)/.75;if(t<.4)old=t;let b=sample(oldBody,old),f=sample(oldFeet,old);for(let i=0;i<4;i++)f[3*i]*=.75;b[0]*=.75;b[1]*=.5;
  if(e>=0&&e<def.cycles*4*def.transfer_s){let transfer=Math.floor(e/def.transfer_s),u=e/def.transfer_s-transfer,leg=[0,2,1,3][transfer%4],a=(1-def.swing_s/def.transfer_s)/2;let env=u<a?u/a:u>1-a?(1-u)/a:1;env=Math.max(0,Math.min(1,env));if(leg===3)b[0]+=.012*env;}
  if(t>=def.duration_s-.8){b=[def.cycles*def.stride_m,0,0];for(let i=0;i<4;i++){f[3*i]=b[0];f[3*i+2]=0}}body.push({time_s:t,values:b});feet.push({time_s:t,values:f});}
 p.base_displacements_world_m.keyframes=body;p.displacements_world_m.keyframes=feet;append(def,p)}
fs.writeFileSync(`${d}/candidates.json`,JSON.stringify(catalog,null,2)+'\n');
