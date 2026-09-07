// Measure the real viewer with rendering enabled, separately from worker throughput.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
import {cpus,platform,arch} from 'node:os';
import {chromium} from 'playwright';
const [directory,preset,reportPath]=process.argv.slice(2);assert(directory&&preset&&reportPath);
await mkdir(dirname(reportPath),{recursive:true});
const catalog=JSON.parse(await readFile(resolve(directory,'catalog.json')));
const entry=catalog.presets.find(p=>p.id===preset);assert(entry?.task);
const data=JSON.parse(await readFile(resolve(directory,entry.path)));
const duration=data.config.step_s*data.config.steps;
const steering=Boolean(data.config.policy?.step_reference);
const server=spawn(process.execPath,['web/serve-viewer.mjs',directory,'0']);
const url=await new Promise((resolve,reject)=>{server.stdout.on('data',c=>{const m=String(c).match(/http:\/\/127.0.0.1:\d+/);if(m)resolve(m[0]);});server.once('error',reject);server.once('exit',c=>reject(Error(`server exited ${c}`)));});
let browser;
try{
 browser=await chromium.launch({headless:process.env.HEADED!=='1',...(process.env.CHROME_EXECUTABLE?{executablePath:process.env.CHROME_EXECUTABLE}:{})});
 const page=await browser.newPage({viewport:{width:1440,height:950}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(()=>{
  window.liveProbe={steps:[]};const Original=window.Worker;
  window.Worker=class extends Original{
   constructor(...args){super(...args);this.starts=new Map();this.addEventListener('message',({data})=>{
    if(data.progress)return;const start=this.starts.get(data.id);if(start!=null){window.liveProbe.steps.push((performance.now()-start)/1000);this.starts.delete(data.id);}
   });}
   postMessage(data,...args){if(data.type==='step')this.starts.set(data.id,performance.now());return super.postMessage(data,...args);}
  };
 });
 await page.goto(`${url}/?preset=${encodeURIComponent(preset)}`);
 await page.locator('#overlay').waitFor({state:'hidden',timeout:30000});
 assert.equal(await page.locator('#preset').inputValue(),preset);
 await page.evaluate(({steering})=>{
  const p=window.liveProbe;p.started=performance.now();p.frames=[];p.previous=p.started;p.running=true;
  let stage=0;const key=(type,key)=>window.dispatchEvent(new KeyboardEvent(type,{key,bubbles:true,cancelable:true}));
  if(steering)key('keydown','w');
  const draw=now=>{if(!p.running)return;p.frames.push((now-p.previous)/1000);p.previous=now;requestAnimationFrame(draw);};requestAnimationFrame(draw);
  p.observer=new MutationObserver(()=>{
   const time=parseFloat(document.querySelector('#sim-time').textContent);
   if(steering&&stage===0&&time>=8.4){key('keyup','w');key('keydown','a');stage=1;}
   if(steering&&stage===1&&time>=16.8){key('keyup','a');key('keydown','s');stage=2;}
   if(steering&&stage===2&&time>=20){key('keyup','s');stage=3;}
   if(/complete|Episode time limit reached|error/i.test(document.querySelector('#execution-state').textContent)){
   p.ended=performance.now();p.running=false;p.observer.disconnect();
  }});p.observer.observe(document.querySelector('#execution-state'),{childList:true,subtree:true});
  document.querySelector('#play').click();
 },{steering});
 await page.waitForFunction(()=>window.liveProbe.ended!=null,null,{timeout:180000});
 const result=await page.evaluate(()=>{
  const p=window.liveProbe,canvas=document.querySelector('canvas'),gl=canvas.getContext('webgl2');const ext=gl?.getExtension('WEBGL_debug_renderer_info');
  return {wall_s:(p.ended-p.started)/1000,simulated_s:parseFloat(document.querySelector('#sim-time').textContent),status:document.querySelector('#execution-state').textContent,
   worker_transitions_s:p.steps,render_intervals_s:p.frames,gpu:ext?gl.getParameter(ext.UNMASKED_RENDERER_WEBGL):null};
 });
 const p95=a=>[...a].sort((a,b)=>a-b)[Math.ceil(a.length*.95)-1];
 const completed=Math.abs(result.simulated_s-duration)<1e-9&&!errors.length
  &&result.worker_transitions_s.length===Math.round(duration/data.task.period_s);
 const performance={simulation_per_wall_second:result.simulated_s/result.wall_s,transition_p95_s:p95(result.worker_transitions_s),render_interval_p95_s:p95(result.render_intervals_s),render_frames:result.render_intervals_s.length,
  transitions:result.worker_transitions_s.length,wall_s:result.wall_s,simulated_s:result.simulated_s};
 const report={completed,preset,performance,meets_speed_target:performance.simulation_per_wall_second>=1,meets_transition_target:performance.transition_p95_s<=.02,
  host:{cpu:cpus()[0]?.model,logical_cpus:cpus().length,platform:platform(),architecture:arch(),browser:await browser.version(),gpu:result.gpu,headless:process.env.HEADED!=='1'},errors,status:result.status,
  scope:'One episode through the actual viewer with WebGL drawing enabled. Online-step presets exercise W, A, S and key release through the UI. rAF intervals measure scheduling, not display presentation. No sustained terrain or command-to-visible-response acceptance; report failures rather than dropping frames or loosening physics.'};
 if(steering&&completed){
  assert.match(await page.locator('#motion-progress').textContent(),/Standing/);
  const download=page.waitForEvent('download');await page.locator('#download').click();const file=await download;await file.saveAs(reportPath.replace(/\.json$/,'.recording.json'));
  const record=JSON.parse(await readFile(reportPath.replace(/\.json$/,'.recording.json'))),events=record.runtime.input_events;
  assert(events.some(e=>e.values[3]>0)&&events.some(e=>e.values[3]<0)&&events.some(e=>e.values[5]>0));
  assert.deepEqual(events.at(-1).values.slice(3),[0,0,0]);
  report.keyboard_commands_recorded=true;
 }
 await page.screenshot({path:reportPath.replace(/\.json$/,'.png')});
 await writeFile(reportPath,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));assert(completed);
}finally{await browser?.close();server.kill();}
