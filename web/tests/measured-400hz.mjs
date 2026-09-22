// Rendered verification against the actual Rust worker, without foreground windows.
import {chromium} from 'playwright';
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
const out=path.resolve(process.env.VERIFICATION_DIR||'examples/full-robot/measured-actuator-integration/browser-control-400hz/verification');
fs.mkdirSync(out,{recursive:true});const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.addInitScript(()=>{
 window.__probe={actions:[],frames:[],errors:[]};const Native=window.Worker;
 window.Worker=class extends Native{constructor(...a){super(...a);this.addEventListener('message',e=>{let v=e.data.result_json??e.data.result;if(typeof v==='string'){try{v=JSON.parse(v);}catch{}}
 if(e.data.error)window.__probe.errors.push(e.data.error);const frame=v?.frame??v;
 if(frame?.time_s!==undefined)window.__probe.frames.push({id:e.data.id,wall_ms:performance.now(),time_s:frame.time_s,error:frame.error,done:frame.done,speed:frame.learning?.speed,completed_steps:frame.completed_steps});
 });}postMessage(m,...rest){if(m.type==='step')window.__probe.actions.push({id:m.id,wall_ms:performance.now(),action:[...m.action]});return super.postMessage(m,...rest);}};
});
try{
 await page.goto(`${process.env.VIEWER_URL||'http://127.0.0.1:4182'}/?preset=${process.env.VIEWER_PRESET||'robot-measured-400hz'}`);
 await page.waitForFunction(()=>!document.querySelector('#play').disabled,null,{timeout:180000});
 await page.screenshot({path:path.join(out,'loaded.png')});
 await page.locator('#play').click();
 for(const key of ['w','a','d','s',null]){
  const index=await page.evaluate(()=>window.__probe.actions.length);if(key)await page.keyboard.down(key);
  await page.waitForFunction(({index,key})=>window.__probe.actions.slice(index).some(a=>key==='w'?a.action[3]>0&&a.action[5]===0:key==='a'?a.action[5]>0:key==='d'?a.action[5]<0:key==='s'?a.action[3]<0:a.action[3]===0&&a.action[5]===0),{index,key},{timeout:180000});
  const id=await page.evaluate(()=>window.__probe.actions.at(-1).id);
  await page.waitForFunction(id=>window.__probe.frames.some(f=>f.id===id),id,{timeout:180000});
  if(key)await page.keyboard.up(key);
 }
 await page.locator('#play').click();
 await page.waitForFunction(()=>window.__probe.frames.some(f=>f.id===window.__probe.actions.at(-1).id),null,{timeout:180000});
 await page.screenshot({path:path.join(out,'wasd.png')});
}catch(e){errors.push(e.message);await page.screenshot({path:path.join(out,'failure.png')});}
const result={errors,readiness:await page.locator('#readiness').textContent(),status:await page.locator('#status').textContent().catch(()=>null),actuation:await page.locator('#actuation-profile').textContent(),performance:await page.locator('#performance').textContent(),speed:await page.locator('#travel-speed').textContent(),probe:await page.evaluate(()=>window.__probe)};
fs.writeFileSync(path.join(out,'browser.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({...result,probe:{actions:result.probe.actions.length,frames:result.probe.frames.length,errors:result.probe.errors}},null,2));await browser.close();assert.equal(errors.length,0);assert.equal(result.probe.errors.length,0);assert(result.probe.frames.at(-1).time_s>=0.1);
