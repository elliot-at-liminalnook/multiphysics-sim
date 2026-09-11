import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
const root=path.dirname(fileURLToPath(import.meta.url));
const percentile=(a,q)=>{const b=a.filter(Number.isFinite).sort((a,b)=>a-b);return b.length?b[Math.min(b.length-1,Math.floor(q*(b.length-1)))]:null};
const average=a=>a.length?a.reduce((s,x)=>s+x,0)/a.length:null;
function slope(rows){const t=rows.map(r=>(r.request_s+r.completion_s)/2),p=rows.map(r=>r.position_raw),mt=average(t),mp=average(p);const d=t.reduce((s,x)=>s+(x-mt)**2,0);return d?rows.reduce((s,r,i)=>s+(t[i]-mt)*(p[i]-mp),0)/d:null;}
function analyze(directory){
 const run=JSON.parse(fs.readFileSync(path.join(root,directory,'run.json')));const pre=JSON.parse(fs.readFileSync(path.join(root,directory,'preflight.json')));
 let lines=fs.readFileSync(path.join(root,directory,'motion.csv'),'utf8').trim().split('\n');const keys=lines.shift().split(',');const rows=lines.map(l=>Object.fromEntries(l.split(',').map((v,i)=>[keys[i],+v]))).filter(r=>Number.isFinite(r.status));
 const groups=new Map();for(const r of rows){let k=[r.stage,r.id,r.repetition,r.move_index].join('/');if(!groups.has(k))groups.set(k,[]);groups.get(k).push(r)}
 const segments=[];
 for(const [key,a] of groups){const r=a[0],stage=pre.plan.stages[r.stage],home=pre.homes[r.id],start=r.move_index===0?home:r.move_index===1?home+stage.amplitude_counts:home-stage.amplitude_counts,target=r.target_raw,delta=target-start;
  const progress=x=>(x.position_raw-start)/delta;const moving=a.filter(x=>progress(x)>=.2&&progress(x)<=.8);let v=slope(moving);const middle_count=moving.length;
  let stable=null;for(let i=0;i<a.length;i++){if(a.at(-1).completion_s-a[i].request_s<.15)break;if(a.slice(i).every(x=>Math.abs(x.position_raw-target)<=12)){stable=a[i].completion_s-r.command_start_s;break}}
  const first10=a.find(x=>progress(x)>=.1),first90=a.find(x=>progress(x)>=.9);
  const overshoot=Math.max(0,...a.map(x=>(x.position_raw-target)*Math.sign(delta)))*360/4096;
  segments.push({key,stage:r.stage,name:stage.name,id:r.id,repetition:r.repetition,move_index:r.move_index,direction:Math.sign(delta),travel_deg:Math.abs(delta)*360/4096,commanded_speed_deg_s:r.speed_limit_raw*360/4096,samples:a.length,middle_fit_samples:middle_count,midtravel_speed_deg_s:v===null?null:Math.abs(v)*360/4096,reported_speed_p95_deg_s:percentile(moving.map(x=>Math.abs(x.speed_rad_s)*180/Math.PI),.95),rise_10_90_s:first10&&first90?first90.completion_s-first10.completion_s:null,settle_within_1p05deg_s:stable,overshoot_deg:overshoot,final_error_deg:(a.at(-1).position_raw-target)*360/4096,voltage_min:Math.min(...a.map(x=>x.voltage_v)),temperature_max:Math.max(...a.map(x=>x.temperature_c)),current_raw_max:Math.max(...a.map(x=>x.current_raw)),poll_interval_median_ms:percentile(a.slice(1).map((x,i)=>(x.request_s-a[i].request_s)*1000),.5)});
 }
 const per_servo=[];for(let id=4;id<=12;id++){
  const a=rows.filter(r=>r.id===id),ss=segments.filter(s=>s.id===id),by_stage=[];
  for(let stage=0;stage<pre.plan.stages.length;stage++){const s=ss.filter(x=>x.stage===stage);if(!s.length)continue;by_stage.push({stage,name:pre.plan.stages[stage].name,commanded_speed_deg_s:pre.plan.stages[stage].speed_counts_s*360/4096,positive_midtravel_speed_deg_s:percentile(s.filter(x=>x.direction>0&&x.middle_fit_samples>=5).map(x=>x.midtravel_speed_deg_s),.5),negative_midtravel_speed_deg_s:percentile(s.filter(x=>x.direction<0&&x.middle_fit_samples>=5).map(x=>x.midtravel_speed_deg_s),.5),reported_speed_p95_deg_s:percentile(s.map(x=>x.reported_speed_p95_deg_s),.5),worst_settle_s:Math.max(...s.map(x=>x.settle_within_1p05deg_s??0)),max_overshoot_deg:Math.max(...s.map(x=>x.overshoot_deg)),current_raw_max:Math.max(...s.map(x=>x.current_raw_max))})}
  per_servo.push({id,samples:a.length,by_stage,voltage_min:Math.min(...a.map(x=>x.voltage_v)),voltage_max:Math.max(...a.map(x=>x.voltage_v)),temperature_min:Math.min(...a.map(x=>x.temperature_c)),temperature_max:Math.max(...a.map(x=>x.temperature_c)),current_raw_max:Math.max(...a.map(x=>x.current_raw)),fault_samples:a.filter(x=>x.status!==0).length});
 }
 const summary={directory,completed:run.completed,failure:run.result?.failure,samples:rows.length,segments,per_servo,method:'Linear regression of position against host transaction midpoints over 20–80% commanded travel. Reported-speed P95 is independent servo register feedback over the same interval. Settling uses ±12 counts (1.055°) through the remaining dwell, requiring at least 150 ms of tail. Overshoot is directional beyond commanded target. No sub-millisecond latency/acceleration claims.'};
 fs.writeFileSync(path.join(root,directory,'analysis.json'),JSON.stringify(summary,null,2)+'\n');
 return summary;
}
let summaries=[];for(const d of process.argv.slice(2)){const s=analyze(d);summaries.push(s);console.log(d,s.samples,'samples, completed',s.completed);for(const p of s.per_servo){console.log(p.id,p.by_stage.map(s=>`${s.name}: ${s.positive_midtravel_speed_deg_s?.toFixed(1)}/${s.negative_midtravel_speed_deg_s?.toFixed(1)} deg/s`).join('; '));}}
