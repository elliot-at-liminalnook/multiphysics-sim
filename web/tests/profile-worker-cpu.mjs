// Sample the actual dedicated worker after initialization. Timing with this
// instrumentation is diagnostic; use the ordinary replay for acceptance.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {chromium} from 'playwright';
const dir=path.resolve(process.argv[2]),prefix=process.argv[3]||'cpu';
const read=name=>JSON.parse(fs.readFileSync(path.join(dir,`${name}.json`)));
const recipe={scene:read('scene'),config:read('config'),task:read('task')},actions=read('actions');
assert(!fs.existsSync(path.join(dir,`${prefix}.cpuprofile`)),'refusing to overwrite a CPU profile');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
const events=[],pending=new Map();let commandId=0,workerSession;
try {
 const page=await browser.newPage(),cdp=await page.context().newCDPSession(page);
 const version=await cdp.send('Browser.getVersion');
 cdp.on('Target.attachedToTarget',event=>{events.push({type:'attached',...event});if(event.targetInfo.type==='worker')workerSession=event.sessionId;});
 cdp.on('Target.receivedMessageFromTarget',event=>{
  const message=JSON.parse(event.message),key=`${event.sessionId}:${message.id}`,request=pending.get(key);
  if(request){pending.delete(key);clearTimeout(request.timer);message.error?request.reject(Error(JSON.stringify(message.error))):request.resolve(message.result);}
 });
 const send=async(method,params={})=>new Promise((resolve,reject)=>{
  const id=++commandId,key=`${workerSession}:${id}`;
  const timer=setTimeout(()=>{pending.delete(key);reject(Error(`worker protocol timeout: ${method}`));},30000);
  pending.set(key,{resolve,reject,timer});
  cdp.send('Target.sendMessageToTarget',{sessionId:workerSession,message:JSON.stringify({id,method,params})}).catch(error=>{clearTimeout(timer);pending.delete(key);reject(error);});
 });
 await cdp.send('Target.setAutoAttach',{autoAttach:true,waitForDebuggerOnStart:false,flatten:false});
 await page.goto((process.env.VIEWER_URL||'http://127.0.0.1:4192')+'/catalog.json');
 const initial=await page.evaluate(async recipe=>{
  const worker=new Worker('/worker.js',{type:'module'}),pending=new Map();let id=0;
  worker.onmessage=({data})=>{const p=pending.get(data.id);if(!p)return;pending.delete(data.id);data.error?p.reject(Error(data.error)):p.resolve(data);};
  const call=message=>new Promise((resolve,reject)=>{message.id=++id;pending.set(id,{resolve,reject});worker.postMessage(message);});
  window.__cpuReplay={worker,call};
  return (await call({type:'load',...recipe,seed:0})).result;
 },recipe);
 assert(workerSession,'dedicated worker session was not attached');
 await send('Profiler.enable');
 await send('Profiler.setSamplingInterval',{interval:1000});
 await send('Profiler.start');
 const result=await page.evaluate(async({actions,initial})=>{
  const {call}=window.__cpuReplay,frames=[initial.frame],timings=[],wall=[];let error;
  try{for(const action of actions){const start=performance.now(),reply=await call({type:'step',action,profile_timing:true});wall.push((performance.now()-start)/1000);frames.push(reply.result);timings.push(reply.timing);if(reply.result.error||reply.result.done){error=reply.result.error;break;}}}catch(e){error=e.message;}
  return {frames,metadata:initial.metadata,timings,transition_wall_s:wall,wall_s:wall.reduce((a,b)=>a+b,0),error};
 },{actions,initial});
 const {profile}=await send('Profiler.stop');
 fs.writeFileSync(path.join(dir,`${prefix}.cpuprofile`),JSON.stringify(profile));
 fs.writeFileSync(path.join(dir,`${prefix}.wasm.json`),JSON.stringify(result));
 fs.writeFileSync(path.join(dir,`${prefix}.metadata.json`),JSON.stringify({version,sampling_interval_us:1000,events,scope:'Dedicated-worker CPU samples after initial model load, during action replay only. Includes worker waiting, serialization and message handling. Profiled wall time is not throughput acceptance.'},null,2)+'\n');
 console.log(JSON.stringify({samples:profile.samples?.length,nodes:profile.nodes.length,simulated_s:result.frames.at(-1).time_s,profiled_wall_s:result.wall_s,error:result.error}));
 assert(!result.error&&result.frames.length===actions.length+1&&result.frames.at(-1).time_s===3);
} catch(error) {
 fs.writeFileSync(path.join(dir,`${prefix}.error.json`),JSON.stringify({error:String(error),events},null,2)+'\n');throw error;
} finally {for(const request of pending.values())clearTimeout(request.timer);await browser.close();}
