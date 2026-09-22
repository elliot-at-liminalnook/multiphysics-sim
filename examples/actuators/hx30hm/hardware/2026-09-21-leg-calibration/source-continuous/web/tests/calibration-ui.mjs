// Synthetic browser contract test: no physical server, serial port, or motor writes.
import {chromium} from 'playwright';import fs from 'node:fs/promises';import assert from 'node:assert/strict';
import {motionComparison} from '../viewer/actuator-motion-view.mjs';
assert.equal(motionComparison({lower:3000,upper:1000},{requested_counts:-10,actual_counts:-9}).agreement,'Measured direction matches command');
assert.match(motionComparison({reverse:true},{requested_counts:-10,actual_counts:9}).agreement,/MISMATCH/);
const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
const until=async f=>{const end=Date.now()+6000;while(!f()){if(Date.now()>end)throw Error('condition timeout');await new Promise(r=>setTimeout(r,20));}};
try{
 const page=await browser.newPage();let commands=[],activeJogs=0,maxJogs=0,interruptNext=false;
 let state={connected:false,enabled_id:null,samples:{3:{position_raw:2048,voltage_v:12,temperature_c:25,current_raw:0}},calibration:{axes:{3:{role:'belt/hip',lower:null,upper:null,reverse:false}}},message:'Ready',output:'fixture-test'};
 await page.route('http://localhost:49999/**',async r=>{
  const url=new URL(r.request().url());let body,type='application/json';
  if(url.pathname==='/'){body='<header></header><meta name="calibration-token" content="test"><script type="module" src="/calibration-ui.mjs"></script>';type='text/html'}
  else if(['/calibration-ui.mjs','/actuator-motion-view.mjs'].includes(url.pathname)){body=await fs.readFile(new URL('../viewer'+url.pathname,import.meta.url),'utf8');type='text/javascript'}
  else if(url.pathname==='/calibration/status')body=JSON.stringify(state);
  else if(url.pathname==='/calibration/command'){
   const c=r.request().postDataJSON();commands.push(c);
   if(c.action==='inspect')state.connected=true;
   if(c.action==='enable')state.enabled_id=c.id;
   if(c.action==='stop')state.enabled_id=null;
   if(c.action==='direction')state.calibration.axes[3].reverse=c.reverse;
   if(c.action==='jog'){
    activeJogs++;maxJogs=Math.max(maxJogs,activeJogs);await new Promise(r=>setTimeout(r,120));
    const initial=state.samples[3].position_raw,actual=interruptNext?c.delta*3:c.delta;state.samples[3].position_raw+=actual;
    state.last_jog={motor_id:3,start_position_raw:initial,requested_counts:c.delta,actual_counts:actual,samples:[{elapsed_ms:0,position_raw:initial},{elapsed_ms:100,position_raw:initial+actual}]};if(interruptNext){state.enabled_id=null;state.last_jog.motion_error="Encoder left commanded jog window";interruptNext=false;}activeJogs--;
   }
   if(c.action==='capture')state.calibration.axes[3][c.boundary]=state.samples[3].position_raw;
   body=JSON.stringify(state);
  }else body='{}';await r.fulfill({status:200,contentType:type,body});
 });
 await page.goto('http://localhost:49999/');await page.waitForFunction(()=>document.querySelector('#fixture-position')?.textContent.includes('2048'));assert.equal(commands.length,0);
 const control=id=>page.locator('#fixture-'+id);
 const ready=()=>page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled&&!document.querySelector('#fixture-drive').disabled);
 await page.locator('#fixture-manual summary').click();await page.locator('#fixture-bounds-editor summary').click();
 assert.equal(await control('plus').isEnabled(),false);
 await control('inspect').click();await page.waitForFunction(()=>!document.querySelector('#fixture-inspect').disabled);assert.equal(await control('enable').isEnabled(),false);
 await control('supported').check();await control('enable').click();await ready();
 await control('drive').fill('37.8');await control('step').fill('17');await control('plus').click();await ready();
 assert.equal(commands.at(-1).drive_pwm,378);assert.equal(commands.at(-1).delta,17);
 assert.match(await control('motion').textContent(),/Measured direction matches command/);
 let count=commands.length;await control('drive').fill('100.1');await control('plus').click();assert.equal(commands.length,count);
 await control('drive').fill('100');await control('step').fill('4096');await control('plus').click();assert.equal(commands.length,count);
 await control('step').fill('17');await control('drive').fill('12.3');
 await control('direction').selectOption('-1');await ready();await control('plus').click();await ready();
 assert.equal(commands.at(-1).delta,-17);assert.equal(commands.at(-1).drive_pwm,123);assert.match(await control('motion').textContent(),/encoder −/);
 const jogCount=()=>commands.filter(c=>c.action==='jog').length;
 const before=jogCount();const box=await control('minus').boundingBox();await page.mouse.move(box.x+20,box.y+20);await page.mouse.down();
 await until(()=>jogCount()>=before+3);await page.mouse.up();await until(()=>commands.at(-1).action==='halt');await ready();
 const after=jogCount();await new Promise(r=>setTimeout(r,500));assert.equal(jogCount(),after);assert.equal(maxJogs,1);
 // Reversed mounting accepts physically named poses in descending encoder order.
 await control('lower').click();await ready();const lower=state.calibration.axes[3].lower;
 await control('plus').click();await ready();await control('upper').click();await ready();assert.ok(state.calibration.axes[3].upper<lower);
 assert.match(await control('bounds').textContent(),/Part travel: 100.0%/);assert.equal(await control('direction').isEnabled(),false);
 await control('stop').click();await page.waitForFunction(()=>document.querySelector('#fixture-plus').disabled&&!document.querySelector('#fixture-axis').disabled);
 assert.ok(commands.filter(c=>c.action!=='inspect'&&c.action!=='enable').every((c,i,a)=>!i||c.sequence>a[i-1].sequence));
 await control('enable').click();await ready();await control('step').fill('20');interruptNext=true;
 await control('plus').click();await until(()=>state.last_jog?.motion_error);
 await page.waitForFunction(()=>document.querySelector('#fixture-motion').textContent.includes('60 counts'));
 assert.equal(await control('plus').isEnabled(),false);
 await page.screenshot({path:'/tmp/calibration-ui-v4.png',fullPage:true});
 console.log('PASS no load motion; arbitrary PWM/count validation; reversed part direction; requested/measured chart; hold repeats serially; release halts with no queued jogs; named endpoints; stop disables movement');
}finally{await browser.close()}
