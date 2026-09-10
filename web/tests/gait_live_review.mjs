// Exercise declared motion keys in the real rendered viewer and retain timing/replay.
import {chromium} from 'playwright';import {spawn} from 'node:child_process';import fs from 'node:fs';import {decodeWorkerResult} from '../worker-message.mjs';
const [bundle,preset,output,schedulePath]=process.argv.slice(2),schedule=JSON.parse(fs.readFileSync(schedulePath));
const server=spawn(process.execPath,['web/serve-viewer.mjs',bundle,'0']);
const url=await new Promise((resolve,reject)=>{server.stdout.on('data',c=>{let m=String(c).match(/http:\/\/127.0.0.1:\d+/);if(m)resolve(m[0])});server.once('error',reject)});
let browser;try{
 browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_EXECUTABLE});let page=await browser.newPage({viewport:{width:1440,height:950}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript({content:`window.decodeGaitWorkerResult=${decodeWorkerResult.toString()};`});
 await page.addInitScript(schedule=>{
  window.gaitProbe={samples:[],commands:[],last_time:0,stage:0,held:[],schedule};let Original=window.Worker;
  window.Worker=class extends Original{constructor(...args){super(...args);window.gaitWorker=this;this.starts=new Map();this.addEventListener('message',({data})=>{
   if(data.progress)return;window.decodeGaitWorkerResult(data);let result=data.result,start=this.starts.get(data.id);if(start!==undefined){this.starts.delete(data.id);let p=window.gaitProbe,now=performance.now();if(result?.time_s!==undefined){p.last_time=result.time_s;p.samples.push({time_s:result.time_s,received_ms:now,wall_s:(now-start)/1000,body_position_m:p.schedule.body_link?result.poses?.find(pose=>pose.name===p.schedule.body_link)?.position_m:undefined,...data.timing});p.last=result;
    while(p.stage<p.schedule.events.length&&result.time_s>=p.schedule.events[p.stage].time_s-1e-9){let e=p.schedule.events[p.stage++];for(let k of p.held)window.dispatchEvent(new KeyboardEvent('keyup',{key:k,bubbles:true}));for(let k of e.keys)window.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true}));p.held=e.keys;p.commands.push({requested_time_s:e.time_s,applied_after_time_s:result.time_s,keys:e.keys});}
   }if(data.error)p.error=data.error;}
  })}postMessage(data,...args){if(data.type==='step'){this.starts.set(data.id,performance.now());data={...data,profile_timing:true,response_encoding:'json'}}return super.postMessage(data,...args)}};
 },schedule);
 await page.goto(`${url}/?preset=${encodeURIComponent(preset)}`);await page.locator('#overlay').waitFor({state:'hidden',timeout:30000});await page.locator('#teleop').waitFor({state:'visible'});await page.locator('#play').click();
 await page.waitForFunction(()=>window.gaitProbe.last_time>=window.gaitProbe.schedule.duration_s-1e-9||window.gaitProbe.error,{},{timeout:150000});
 await page.screenshot({path:output+'.png'});
 const p=await page.evaluate(async()=>{let p=window.gaitProbe;let record=await new Promise((resolve,reject)=>{let id=900000001;let h=({data})=>{if(data.id!==id)return;window.gaitWorker.removeEventListener('message',h);data.error?reject(Error(data.error)):resolve(data.result)};window.gaitWorker.addEventListener('message',h);window.gaitWorker.postMessage({type:'recording',id})});let viewer=await import('./viewer.js');return {...p,record,rendered_frames:viewer.renderedFrameCount()}});
 const active=p.samples.filter(s=>s.time_s>=schedule.active_window_s[0]&&s.time_s<=schedule.active_window_s[1]);let times=active.map(s=>s.wall_s).sort((a,b)=>a-b);const result={version:1,completed:p.last_time>=schedule.duration_s,error:p.error??null,page_errors:errors,rendered_frames:p.rendered_frames,commands:p.commands,active_window_s:[active[0].time_s,active.at(-1).time_s],simulation_per_wall:(active.at(-1).time_s-active[0].time_s)/((active.at(-1).received_ms-active[0].received_ms)/1000),p95_transition_s:times[Math.ceil(.95*times.length)-1],scope:'Rendered live Rust/Rhai physics via production worker, controlled by UI keys. No playback. Transition timing includes worker response transport and decode; active wall pace includes rendering/scheduling. Independent physical acceptance and replay/native parity remain separate.'};
 if(schedule.body_link){
  const a=active[0].body_position_m,b=active.at(-1).body_position_m;
  result.active_body_displacement_m=a&&b?Math.hypot(...b.map((v,i)=>v-a[i])):null;
  result.minimum_active_displacement_m=schedule.minimum_active_displacement_m;
  result.body_motion_verified=Number.isFinite(schedule.minimum_active_displacement_m)&&schedule.minimum_active_displacement_m>0
   &&result.active_body_displacement_m!==null&&result.active_body_displacement_m>=schedule.minimum_active_displacement_m;
 }
 fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n');fs.writeFileSync(output+'.recording.json',JSON.stringify(p.record)+'\n');fs.writeFileSync(output+'.timing.json',JSON.stringify(p.samples)+'\n');console.log(result);
 if(schedule.body_link&&!result.body_motion_verified)throw Error('declared active body motion was not observed');
}finally{await browser?.close();server.kill()}
