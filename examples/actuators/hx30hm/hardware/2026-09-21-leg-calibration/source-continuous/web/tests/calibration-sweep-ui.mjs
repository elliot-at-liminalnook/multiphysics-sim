// Browser control contract; all endpoints mocked, no hardware requests.
import {chromium} from 'playwright';import fs from 'node:fs/promises';import assert from 'node:assert/strict';
const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
const until=async f=>{const end=Date.now()+5000;while(!f()){if(Date.now()>end)throw Error('condition timeout');await new Promise(r=>setTimeout(r,20));}};
try{
 const page=await browser.newPage();let commands=[],run=0;
 const state={connected:true,enabled_id:null,calibration:{axes:{3:{lower:3000,upper:1000,reference:2000,reverse:true}}},samples:{3:{position_raw:2000,voltage_v:12,temperature_c:25,current_raw:0}},message:'Ready'};
 await page.route('http://localhost:49998/**',async route=>{
  const path=new URL(route.request().url()).pathname;let body,type='application/json';
  if(path==='/'){body='<header></header><meta name="calibration-token" content="test"><script type="module" src="/calibration-ui.mjs"></script>';type='text/html';}
  else if(path.endsWith('.mjs')){body=await fs.readFile(new URL('../viewer'+path,import.meta.url),'utf8');type='text/javascript';}
  else if(path==='/calibration/command'){
   const c=route.request().postDataJSON();commands.push(c);
   if(c.action==='enable')state.enabled_id=3;
   if(c.action==='sweep_start')state.sweep={running:true,run_id:++run,motor_id:3,samples:[]};
   if(c.action==='sweep_update'){
    assert.equal(c.run_id,run);assert.equal(state.sweep.running,true);
    const latest={elapsed_ms:commands.length*100,position_raw:1999,target_raw:1998.5,velocity_counts_s:-5,requested_speed_counts_s:c.speed_counts_s,pwm:-100,toward_upper:true,half_cycles:0};
    state.sweep.latest=latest;state.sweep.samples=[latest];
   }
   if(c.action==='stop'||c.action==='clear'){state.enabled_id=null;if(state.sweep)state.sweep.running=false;}
   if(c.action==='clear'){const a=state.calibration.axes[3];if(c.boundary==='both'){a.lower=null;a.upper=null}else a[c.boundary]=null;}
   body=JSON.stringify(c.action==='sweep_update'?{ok:true}:state);
  }else body=JSON.stringify(state);
  await route.fulfill({status:200,contentType:type,body});
 });
 await page.goto('http://localhost:49998/');const el=id=>page.locator('#fixture-'+id);
 await page.waitForFunction(()=>document.querySelector('#fixture-position').textContent.includes('2000'));assert.equal(commands.length,0);
 await el('supported').check();await el('enable').click();await until(()=>state.enabled_id===3);
 await el('sweep-speed').fill('300');await el('sweep-start').click();await until(()=>commands.some(c=>c.action==='sweep_update'));
 assert.equal(await el('sweep-speed').inputValue(),'5');assert.equal(commands.find(c=>c.action==='sweep_update').speed_counts_s,5);assert.equal(commands.find(c=>c.action==='sweep_start').clearance_counts,64);
 await el('faster').click();await until(()=>commands.some(c=>c.action==='sweep_update'&&c.speed_counts_s===10));
 await el('slower').click();await until(()=>commands.at(-1).action==='sweep_update'&&commands.at(-1).speed_counts_s===5);
 await el('sweep-speed').fill('2.3');await el('sweep-speed').press('Tab');await until(()=>commands.at(-1).speed_counts_s===2.3);
 assert.equal(await el('plus').isEnabled(),false);assert.equal(await el('upper').isEnabled(),false);
 await page.waitForFunction(()=>document.querySelector('#fixture-sweep-readout').textContent.includes('measured -5.0'));
 await page.locator('#fixture-bounds-editor summary').click();await el('clear-both').click();await until(()=>state.calibration.axes[3].lower===null);
 assert.equal(state.calibration.axes[3].reference,2000);await new Promise(r=>setTimeout(r,400));assert.equal(commands.at(-1).action,'clear');assert.equal(await el('sweep-start').isEnabled(),false);
 // Starting another run needs explicit enable; blur cancels its heartbeat and sends Stop.
 state.calibration.axes[3].lower=3000;state.calibration.axes[3].upper=1000;
 await el('enable').click();await page.waitForFunction(()=>!document.querySelector('#fixture-sweep-start').disabled);await el('sweep-start').click();await until(()=>commands.at(-1).action==='sweep_update');
 await page.evaluate(()=>window.dispatchEvent(new Event('blur')));await until(()=>commands.at(-1).action==='stop');await new Promise(r=>setTimeout(r,400));assert.equal(commands.at(-1).action,'stop');
 assert.equal(commands.some(c=>c.action==='jog'),false);
 await page.screenshot({path:'/tmp/calibration-sweep-ui.png',fullPage:true});
 console.log('PASS continuous start at crawl, live slower/faster and numeric speed, heartbeat, no jog backlog, measured speed, clear bounds stops traversal and preserves reference, blur stops and ends heartbeat');
}finally{await browser.close();}
