// Browser contract test; synthetic telemetry only, no serial server or actuator writes.
import {chromium} from 'playwright';import fs from 'node:fs/promises';import assert from 'node:assert/strict';
const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
try{
 const page=await browser.newPage();let commands=[];
 let state={connected:false,enabled_id:null,samples:{3:{position_raw:2048,voltage_v:12,temperature_c:25,current_raw:0}},calibration:{axes:{3:{role:'belt/hip',lower:null,upper:null}}},message:'Ready',output:'fixture-test'};
 await page.route('http://localhost:49999/**',async r=>{
  const url=new URL(r.request().url());let body,type='application/json';
  if(url.pathname==='/'){body='<header></header><meta name="calibration-token" content="test"><script type="module" src="/calibration-ui.mjs"></script>';type='text/html'}
  else if(url.pathname==='/calibration-ui.mjs'){body=await fs.readFile(new URL('../viewer/calibration-ui.mjs',import.meta.url),'utf8');type='text/javascript'}
  else if(url.pathname==='/calibration/status')body=JSON.stringify(state);
  else if(url.pathname==='/calibration/command'){
   const c=r.request().postDataJSON();commands.push(c);
   if(c.action==='inspect')state.connected=true;
   if(c.action==='enable')state.enabled_id=c.id;
   if(c.action==='stop')state.enabled_id=null;
   if(c.action==='jog')state.samples[3].position_raw+=c.delta;
   if(c.action==='capture')state.calibration.axes[3][c.boundary]=state.samples[3].position_raw;
   body=JSON.stringify(state);
  }else body='{}';await r.fulfill({status:200,contentType:type,body});
 });
 await page.goto('http://localhost:49999/');await page.waitForFunction(()=>document.querySelector('#fixture-position')?.textContent.includes('2048'));assert.equal(commands.length,0);
 assert.equal(await page.locator('#fixture-plus').isEnabled(),false);
 await page.locator('#fixture-inspect').click();await page.waitForFunction(()=>!document.querySelector('#fixture-inspect').disabled);assert.equal(await page.locator('#fixture-enable').isEnabled(),false);
 await page.locator('#fixture-supported').check();await page.locator('#fixture-enable').click();await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled);
 await page.locator('#fixture-plus').click();await page.waitForFunction(()=>document.querySelector('#fixture-position').textContent.includes('2049'));
 await page.locator('#fixture-lower').click();await page.waitForFunction(()=>document.querySelector('#fixture-bounds').textContent.includes('Lower: 2049'));
 await page.locator('#fixture-stop').click();await page.waitForFunction(()=>document.querySelector('#fixture-plus').disabled&&!document.querySelector('#fixture-axis').disabled);
 assert.deepEqual(commands.map(c=>c.action),['inspect','enable','jog','capture','stop']);assert.equal(commands[2].delta,1);assert.equal(commands[2].id,3);assert.equal(commands[2].drive_pwm,100);assert.ok(commands[3].sequence>commands[2].sequence);
 console.log('PASS no movement on load, supported-fixture gate, one-count jog, capture from displayed readback, stop disables jog, monotonically ordered commands');
}finally{await browser.close()}
