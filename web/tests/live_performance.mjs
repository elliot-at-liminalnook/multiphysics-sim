// Measure the real viewer with rendering enabled, separately from worker throughput.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
import {cpus,platform,arch} from 'node:os';
import {createHash} from 'node:crypto';
import {chromium} from 'playwright';
const [directory,preset,reportPath,scenario='turn-reverse',configPath]=process.argv.slice(2);assert(directory&&preset&&reportPath);
await mkdir(dirname(reportPath),{recursive:true});
const catalog=JSON.parse(await readFile(resolve(directory,'catalog.json')));
const entry=catalog.presets.find(p=>p.id===preset);assert(entry?.task);
const data=JSON.parse(await readFile(resolve(directory,entry.path)));
let configOverride=null;
if(configPath){
 const bytes=await readFile(configPath);data.config=JSON.parse(bytes);
 configOverride={path:configPath,sha256:createHash('sha256').update(bytes).digest('hex')};
}
const duration=data.config.step_s*data.config.steps;
const steering=Boolean(data.config.policy?.step_reference);
const schedules={
 'turn-reverse':[[0,'w'],[8.4,'a'],[16.8,'s'],[20,null]],
 'forward-reverse':[[0,'w'],[8.4,'s'],[16.8,null]],
 'reverse-forward':[[0,'s'],[8.4,'w'],[16.8,null]],
 'sustained-forward':[[0,'w'],[duration-4,null]],
};
assert(schedules[scenario],'unknown keyboard scenario');const schedule=schedules[scenario];
const server=spawn(process.execPath,['web/serve-viewer.mjs',directory,'0']);
const url=await new Promise((resolve,reject)=>{server.stdout.on('data',c=>{const m=String(c).match(/http:\/\/127.0.0.1:\d+/);if(m)resolve(m[0]);});server.once('error',reject);server.once('exit',c=>reject(Error(`server exited ${c}`)));});
let browser;
try{
 browser=await chromium.launch({headless:process.env.HEADED!=='1',...(process.env.CHROME_EXECUTABLE?{executablePath:process.env.CHROME_EXECUTABLE}:{})});
 const page=await browser.newPage({viewport:{width:1440,height:950}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(()=>{
  window.liveProbe={steps:[],samples:[],commands:[]};const Original=window.Worker;
  window.Worker=class extends Original{
   constructor(...args){super(...args);this.starts=new Map();this.addEventListener('message',({data})=>{
    if(data.progress)return;const start=this.starts.get(data.id);if(start!=null){const p=window.liveProbe,now=performance.now(),wall=(now-start)/1000;
      p.steps.push(wall);p.samples.push({time_s:data.result?.time_s,wall_s:wall,interval_s:(now-(p.previousResponse??p.started))/1000,phase:data.result?.policy?.step_reference?.reference.phase,...data.timing});p.previousResponse=now;p.finalPhase=data.result?.policy?.step_reference?.reference.phase;
      const reference=data.result?.policy?.step_reference?.reference;
      for(const command of p.commands){
       if(command.superseded||command.reference_response_s!==undefined||!reference)continue;
       if(reference.sample>command.previous_reference_sample&&reference.latched_twist.every((v,i)=>Math.abs(v-command.requested_twist[i])<1e-12)){
        command.reference_response_s=(now-command.issued_at_ms)/1000;
        command.reference_frame_time_s=data.result.time_s;
       }
      }
      this.starts.delete(data.id);}
   });}
   postMessage(data,...args){if(data.type==='step'){this.starts.set(data.id,performance.now());data={...data,profile_timing:true};}return super.postMessage(data,...args);}
  };
 });
 if(configOverride)await page.route(`**/${entry.path}`,route=>route.fulfill({contentType:'application/json',body:JSON.stringify(data)}));
 await page.goto(`${url}/?preset=${encodeURIComponent(preset)}`);
 await page.locator('#overlay').waitFor({state:'hidden',timeout:30000});
 assert.equal(await page.locator('#preset').inputValue(),preset);
 await page.evaluate(async()=>{
  const viewer=await import('./viewer.js');window.liveProbe.readRenderCount=viewer.renderedFrameCount;window.liveProbe.readRenderedFrame=viewer.renderedFrameInfo;
 });
 await page.evaluate(({steering,schedule,commandChannels})=>{
  const p=window.liveProbe;p.initialDraws=p.readRenderCount?.();p.started=performance.now();p.frames=[];p.previous=p.started;p.running=true;
  let stage=0;const key=(type,key)=>window.dispatchEvent(new KeyboardEvent(type,{key,bubbles:true,cancelable:true}));
  const recordCommand=()=>{
   for(const command of p.commands)if(command.reference_response_s===undefined)command.superseded=true;
   const drawn=p.readRenderedFrame?.();
   const sliders=[...document.querySelectorAll('#inputs input')];
   p.commands.push({stage,requested_twist:commandChannels.map(name=>Number(sliders.find(input=>input.getAttribute('aria-label')===name).value)),
    issued_at_ms:performance.now(),issued_at_simulation_s:parseFloat(document.querySelector('#sim-time').textContent),
    previous_reference_sample:drawn?.reference_sample??-1});
  };
  if(steering){key('keydown',schedule[0][1]);recordCommand();}
  const draw=now=>{if(!p.running)return;p.frames.push((now-p.previous)/1000);p.previous=now;
   const rendered=p.readRenderedFrame?.();
   for(const command of p.commands){
    if(command.superseded||command.drawn_reference_s!==undefined||!rendered?.latched_twist)continue;
    if(rendered.submitted_at_ms>=command.issued_at_ms&&rendered.reference_sample>command.previous_reference_sample
     &&rendered.latched_twist.every((v,i)=>Math.abs(v-command.requested_twist[i])<1e-12)){
      command.drawn_reference_s=(rendered.submitted_at_ms-command.issued_at_ms)/1000;
      command.drawn_frame_time_s=rendered.time_s;command.drawn_frame=rendered.frame;
    }
   }
   requestAnimationFrame(draw);
  };requestAnimationFrame(draw);
  p.observer=new MutationObserver(()=>{
   const last=p.samples.at(-1);if(last&&last.view_update_s===undefined)last.view_update_s=(performance.now()-p.previousResponse)/1000;
   const time=parseFloat(document.querySelector('#sim-time').textContent);
   while(steering&&schedule[stage+1]&&time>=schedule[stage+1][0]){
    if(schedule[stage][1])key('keyup',schedule[stage][1]);stage++;
    if(schedule[stage][1])key('keydown',schedule[stage][1]);
    recordCommand();
   }
   if(/complete|Episode time limit reached|error/i.test(document.querySelector('#execution-state').textContent)){
   p.ended=performance.now();p.running=false;p.observer.disconnect();
  }});p.observer.observe(document.querySelector('#execution-state'),{childList:true,subtree:true});
  document.querySelector('#play').click();
 },{steering,schedule,commandChannels:data.config.policy?.step_reference?.command_channels??[]});
 await page.waitForFunction(()=>window.liveProbe.ended!=null,null,{timeout:180000});
 const result=await page.evaluate(()=>{
  const p=window.liveProbe,canvas=document.querySelector('canvas'),gl=canvas.getContext('webgl2');const ext=gl?.getExtension('WEBGL_debug_renderer_info');
  return {wall_s:(p.ended-p.started)/1000,simulated_s:parseFloat(document.querySelector('#sim-time').textContent),status:document.querySelector('#execution-state').textContent,
   worker_transitions_s:p.steps,transition_samples:p.samples,final_phase:p.finalPhase,render_intervals_s:p.frames,
   actual_drawn_frames:p.readRenderCount?p.readRenderCount()-p.initialDraws:null,
   commands:p.commands,drawn_reference_supported:Boolean(p.readRenderedFrame),
   gpu:ext?gl.getParameter(ext.UNMASKED_RENDERER_WEBGL):null};
 });
 const p95=a=>[...a].sort((a,b)=>a-b)[Math.ceil(a.length*.95)-1];
 const completed=Math.abs(result.simulated_s-duration)<1e-9&&!errors.length
  &&result.worker_transitions_s.length===Math.round(duration/data.task.period_s);
 if(steering&&completed&&result.drawn_reference_supported){
  assert.equal(result.commands.length,schedule.length);
  for(const command of result.commands){
   assert(!command.superseded,'scheduled command must reach the controller before its replacement');
   assert(Number.isFinite(command.reference_response_s)&&command.reference_response_s>=0);
   assert(Number.isFinite(command.drawn_reference_s)&&command.drawn_reference_s>=command.reference_response_s);
   assert(command.drawn_frame_time_s>=command.reference_frame_time_s);
  }
 }
 const performance={simulation_per_wall_second:result.simulated_s/result.wall_s,transition_p95_s:p95(result.worker_transitions_s),render_interval_p95_s:p95(result.render_intervals_s),render_frames:result.render_intervals_s.length,
  actual_drawn_frames:result.actual_drawn_frames,
  command_response:{drawn_reference_supported:result.drawn_reference_supported,commands:result.commands,
   scope:'Keyboard dispatch to the first received/drawn frame carrying the requested latched walking reference. Drawing timestamps follow WebGL submission; controller transfer-boundary waits are included. This does not establish physical stopping time, causal body-motion response or monitor presentation latency.'},
  transitions:result.worker_transitions_s.length,wall_s:result.wall_s,simulated_s:result.simulated_s};
 const active=result.transition_samples.filter(s=>s.phase&&s.phase!=='hold'&&s.phase!=='idle');
 if(active.length){const wall=active.reduce((n,s)=>n+s.interval_s,0);performance.active_motion={transitions:active.length,simulated_s:active.length*data.task.period_s,wall_s:wall,simulation_per_wall_second:active.length*data.task.period_s/wall,transition_p95_s:p95(active.map(s=>s.wall_s))};}
 const summarize=samples=>Object.fromEntries(['wall_s','worker_s','wasm_call_s','json_parse_s','queue_s','view_update_s','transport_and_dispatch_s'].map(k=>{
  const values=samples.map(s=>k==='transport_and_dispatch_s'?s.wall_s-s.worker_s-s.queue_s:s[k]).filter(Number.isFinite);
  return [k,{samples:values.length,mean:values.reduce((a,b)=>a+b,0)/values.length,p95:p95(values),maximum:Math.max(...values)}];
 }));
 performance.breakdown={all:summarize(result.transition_samples),by_phase:Object.fromEntries([...new Set(result.transition_samples.map(s=>s.phase))].map(phase=>[phase,summarize(result.transition_samples.filter(s=>s.phase===phase))])),scope:'WASM call includes Rust physics, controller, frame construction and JSON serialization. Transport/dispatch is round-trip minus measured worker time and local queue. View update ends at the DOM mutation observer, before display presentation. Component p95 values are not additive.'};
 const report={completed,preset,scenario,config_override:configOverride,performance,meets_speed_target:performance.simulation_per_wall_second>=1&&(!performance.active_motion||performance.active_motion.simulation_per_wall_second>=1),meets_transition_target:performance.transition_p95_s<=.02&&(!performance.active_motion||performance.active_motion.transition_p95_s<=.02),
  host:{cpu:cpus()[0]?.model,logical_cpus:cpus().length,platform:platform(),architecture:arch(),browser:await browser.version(),gpu:result.gpu,headless:process.env.HEADED!=='1'},errors,status:result.status,
  scope:'One episode through the actual viewer with WebGL drawing enabled. Online-step presets exercise the named keyboard scenario and key release. Active-motion timing excludes hold/idle so standing cannot hide walking latency. rAF intervals measure scheduling, not display presentation. Command-reference delays are measured separately from physical response; no sustained terrain or physical stopping acceptance.'};
 if(steering&&completed){
  assert.match(await page.locator('#motion-progress').textContent(),/Episode ended/);assert.equal(result.final_phase,'idle');
  const download=page.waitForEvent('download');await page.locator('#download').click();const file=await download;await file.saveAs(reportPath.replace(/\.json$/,'.recording.json'));
  const record=JSON.parse(await readFile(reportPath.replace(/\.json$/,'.recording.json'))),events=record.runtime.input_events;
  const channels=data.scene.controller.inputs;
  const motion=data.config.policy.step_reference.command_channels.map(name=>channels.findIndex(c=>c.name===name));
  assert.equal(motion.length,3);assert(motion.every(i=>i>=0));
  assert(events.some(e=>e.values[motion[0]]>0));
  if(scenario!=='sustained-forward')assert(events.some(e=>e.values[motion[0]]<0));
  if(scenario==='turn-reverse')assert(events.some(e=>e.values[motion[2]]>0));
  assert.deepEqual(motion.map(i=>events.at(-1).values[i]),[0,0,0]);
  // Keyboard scenarios must not silently modify gains or policy corrections.
  for(const event of events)for(const [i,channel] of channels.entries()){
   if(!motion.includes(i))assert.equal(event.values[i],channel.initial);
  }
  report.keyboard_commands_recorded=true;
 }
 await page.screenshot({path:reportPath.replace(/\.json$/,'.png')});
 await writeFile(reportPath.replace(/\.json$/,'.timing.json'),JSON.stringify(result.transition_samples)+'\n');
 await writeFile(reportPath,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));assert(completed);
}finally{await browser?.close();server.kill();}
