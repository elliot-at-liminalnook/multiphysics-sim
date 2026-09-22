// Same Rust environment recipe/actions, with independent native and worker receipts.
import fs from'node:fs';import path from'node:path';import{spawnSync}from'node:child_process';import{createHash}from'node:crypto';import{chromium}from'playwright';
const dir=path.resolve(process.argv[2]),mode=process.argv[3]||'both',prefix=process.argv[4]||'baseline';
const read=n=>JSON.parse(fs.readFileSync(path.join(dir,n+'.json')));const recipe={scene:read('scene'),config:read('config'),task:read('task')},actions=read('actions');
if(mode==='native'||mode==='native-fast'||mode==='profile'||mode==='both'){
 const binary=process.env.NATIVE_BINARY||'target/release/examples/run_environment';
 for(const profile of (mode==='native-fast'?[false]:mode==='profile'?[true]:[false,true])){
 const name=prefix+(profile?'.profiled':'.native'),out=fs.openSync(path.join(dir,name+'.json'),'w'),err=fs.openSync(path.join(dir,name+'.log'),'w');
 const args=['scene','config','task','actions'].map(k=>path.join(dir,k+'.json'));if(profile)args.push('--profile',path.join(dir,prefix+'.profile.json'));
 const result=spawnSync(binary,args,{stdio:['ignore',out,err],env:{...process.env,RAYON_NUM_THREADS:'1'}});fs.closeSync(out);fs.closeSync(err);if(result.status!==0)throw Error(`${name} failed: ${result.status}`);
 const r=read(name);console.log(name,JSON.stringify({complete:r.completed,wall_s:r.wall_s,sim_s:r.frames.at(-1).time_s}));
 }
 fs.writeFileSync(path.join(dir,prefix+'.binary.json'),JSON.stringify({path:path.resolve(binary),sha256:createHash('sha256').update(fs.readFileSync(binary)).digest('hex')}));
}
if(mode==='wasm'||mode==='both'){
 const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});const page=await browser.newPage();
 try{await page.goto((process.env.VIEWER_URL||'http://127.0.0.1:4182')+'/catalog.json');
 const r=await page.evaluate(async({recipe,actions})=>{
 const worker=new Worker('/worker.js',{type:'module'}),pending=new Map();let id=0;worker.onmessage=({data})=>{const p=pending.get(data.id);if(!p)return;pending.delete(data.id);data.error?p.reject(Error(data.error)):p.resolve(data);};
 const call=m=>new Promise((resolve,reject)=>{m.id=++id;pending.set(id,{resolve,reject});worker.postMessage(m);});
 const frames=[],timings=[],wall=[];let metadata,error;try{const initial=(await call({type:'load',...recipe,seed:0})).result;metadata=initial.metadata;frames.push(initial.frame);
 for(const action of actions){const start=performance.now();const reply=await call({type:'step',action,profile_timing:true});wall.push((performance.now()-start)/1000);frames.push(reply.result);timings.push(reply.timing);if(reply.result.error||reply.result.done){error=reply.result.error;break;}}
 }catch(e){error=e.message;}finally{worker.terminate();}return {frames,metadata,timings,transition_wall_s:wall,wall_s:wall.reduce((a,b)=>a+b,0),error};
 },{recipe,actions});fs.writeFileSync(path.join(dir,prefix+'.wasm.json'),JSON.stringify(r));console.log(prefix+'.wasm',JSON.stringify({sim_s:r.frames.at(-1).time_s,wall_s:r.wall_s,error:r.error}));if(r.error)throw Error(r.error);
 }finally{await browser.close();}
}
