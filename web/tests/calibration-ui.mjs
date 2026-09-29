// Browser behavior contract with mocked transport; never connects to hardware.
import {chromium} from 'playwright';import fs from 'node:fs/promises';import assert from 'node:assert/strict';
const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
const until=async f=>{const end=Date.now()+7000;while(!f()){if(Date.now()>end)throw Error('condition timeout');await new Promise(r=>setTimeout(r,20));}};
try{
 const page=await browser.newPage({viewport:{width:1360,height:900}});let commands=[],run=0,delayStart=0;
 const state={connected:false,enabled_id:null,calibration:{axes:Object.fromEntries([1,2,3].map(id=>[id,{role:'test',lower:id===2?3000:1000,upper:id===2?1000:3000,reference:2000,reverse:id===2}]))},samples:Object.fromEntries([1,2,3].map(id=>[id,{position_raw:2000,voltage_v:12,temperature_c:25,current_raw:0}])),message:'Ready'};
 await page.route('http://localhost:49999/**',async route=>{
  const path=new URL(route.request().url()).pathname;let body,type='application/json';
  if(path==='/'){body='<header></header><meta name="calibration-token" content="test"><script type="module" src="/calibration-ui.mjs"></script>';type='text/html';}
  else if(path.endsWith('.mjs')){body=await fs.readFile(new URL('../viewer'+path,import.meta.url),'utf8');type='text/javascript';}
  else if(path==='/calibration/command'){
   const c=route.request().postDataJSON();commands.push(c);
   if(c.action==='select'){state.connected=true;state.enabled_id=c.id;state.sweep=null;}
   if(c.action==='motion_start'){state.sweep={running:true,run_id:++run,motor_id:c.id,teaching:true,samples:[]};body=JSON.stringify(state);if(delayStart)await new Promise(r=>setTimeout(r,delayStart));}
   if(c.action==='motion_update'){
    assert.equal(c.run_id,run);if(!state.sweep.running){body=JSON.stringify({ok:true});}else{
    const half=(state.sweep.latest?.half_cycles??0)+(c.motion==='sweep'?1:0);const latest={elapsed_ms:commands.length*100,position_raw:2000,target_raw:2000,velocity_counts_s:0,requested_speed_counts_s:c.speed_counts_s,pwm:15,toward_upper:true,half_cycles:half,holding:c.motion==='hold'};
    state.sweep.latest=latest;state.sweep.samples=[latest];
   }}
   if(c.action==='stop'||c.action==='clear'){state.enabled_id=null;if(state.sweep)state.sweep.running=false;}
   if(c.action==='clear'){const a=state.calibration.axes[c.id];if(c.boundary==='both'){a.lower=null;a.upper=null;}else a[c.boundary]=null;}
   if(c.action==='sweep_all'){const ids=Object.keys(state.calibration.axes).map(Number).filter(k=>!state.calibration.axes[k].disabled);state.sweep={running:true,run_id:++run,motor_id:c.id,motor_ids:ids,skipped:['test (disabled)'],axes:{},samples:[]};body=JSON.stringify(state);}
   if(c.action==='motion_update'&&state.sweep?.motor_ids){for(const k of state.sweep.motor_ids){const h=(state.sweep.axes[k]?.half_cycles??-1)+1;state.sweep.axes[k]={half_cycles:h};}if(Object.values(state.sweep.axes).every(a=>a.half_cycles>=2)){state.sweep.running=false;state.enabled_id=null;}}
   if(c.action==='tune'){assert.equal(c.supported,true,'operator confirmed mid-travel');state.tuning={running:true,motor_id:c.id,stage:'Finding friction',polls:0};body=JSON.stringify(state);}
   if(c.action==='set_disabled'){assert.notEqual(state.enabled_id,c.id,'disable only a stopped motor');state.calibration.axes[c.id].disabled=c.disabled;}
   if(c.action==='capture_hold'){assert.equal(c.motion,'hold');state.calibration.axes[c.id][c.boundary]=2000;state.capture_message='Saved '+c.boundary+' pose';}
   body??=JSON.stringify(['motion_update','capture_hold'].includes(c.action)?{ok:true}:state);
  }else{if(state.tuning?.running&&++state.tuning.polls>2){state.tuning.running=false;state.enabled_id=null;state.calibration.axes[state.tuning.motor_id].tuning={pid:{kp:1.5,ki:2,kd:0.07,integral_limit:0.1,duty_limit:1},friction_duty:0.06,record:'tune-2-1.json'};}body=JSON.stringify(state);}
  await route.fulfill({status:200,contentType:type,body});
 });
 await page.goto('http://localhost:49999/');const el=id=>page.locator('#fixture-'+id);const motor=id=>page.locator(`.motor[data-id="${id}"]`);
 await el('plus').waitFor();assert.equal(commands.length,0,'page load must not command hardware');
 await motor(2).click();await until(()=>state.enabled_id===2);await el('plus').waitFor({state:'visible'});
 await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled);
 assert.deepEqual(commands.map(c=>c.action),['select'],'one selection replaces inspect/confirm/enable');
 delayStart=250;await page.keyboard.down('q');await until(()=>commands.some(c=>c.action==='motion_start'));await page.keyboard.up('q');
 await until(()=>commands.some(c=>c.action==='motion_update'&&c.motion==='hold'));
 assert.equal(commands.some(c=>['stop','halt','jog'].includes(c.action)),false,'key release must keep drive session and hold');
 delayStart=0;const before=commands.length;await page.keyboard.down('a');await until(()=>commands.slice(before).some(c=>c.motion==='lower'));await page.keyboard.up('a');await until(()=>commands.at(-1).motion==='hold');
 assert.equal(commands.filter(c=>c.action==='motion_start').length,1,'direction changes share one session');
 await page.waitForFunction(()=>!document.querySelector('#fixture-upper').disabled);await el('upper').click();await until(()=>commands.some(c=>c.action==='capture_hold'));assert.equal(commands.some(c=>c.action==='stop'),false,'capture in hold must not drop torque');
 await page.locator('#fixture-advanced summary').click();await el('pwm').fill('82.7');await el('pwm').press('Tab');await until(()=>commands.at(-1).drive_pwm===827);
 // Q/A while entering engineering numbers must not command motion.
 const n=commands.filter(c=>c.motion==='upper').length;await el('step').focus();await page.keyboard.press('q');assert.equal(commands.filter(c=>c.motion==='upper').length,n);
 // Z works even with an input focused and latches until motor selection.
 await page.keyboard.press('z');await until(()=>commands.at(-1).action==='stop');const stopped=commands.length;await new Promise(r=>setTimeout(r,350));assert.equal(commands.length,stopped);assert.equal(await el('plus').isDisabled(),true);
 await motor(3).click();await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled);
 await el('sweep').click();await until(()=>commands.at(-1).motion==='sweep');assert.equal(commands.at(-1).speed_counts_s,5);
 await el('sweep').click();await until(()=>commands.at(-1).motion==='hold');
 await el('speed').fill('50');await el('speed').dispatchEvent('input');await until(()=>commands.at(-1).speed_counts_s===50);
 // Learning is an explicit interior trial, uses the same lease and can pause into hold.
 await el('learn').click();await until(()=>commands.at(-1).motion==='learn');
 assert.equal(commands.at(-1).speed_counts_s,50);assert.equal(commands.at(-1).drive_pwm,827);
 await el('learn').click();await until(()=>commands.at(-1).motion==='hold');
 // Switching windows keeps control; closing the page cancels drive and renewal.
 const beforeBlur=commands.length;await page.evaluate(()=>window.dispatchEvent(new Event('blur')));await new Promise(r=>setTimeout(r,300));assert.equal(commands.slice(beforeBlur).some(c=>c.action==='stop'),false,'window blur must not stop');
 await page.evaluate(()=>window.dispatchEvent(new Event('pagehide')));await until(()=>commands.at(-1).action==='stop');const count=commands.length;await new Promise(r=>setTimeout(r,350));assert.equal(commands.length,count);
 await motor(3).click();await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled);await el('reset').click();await until(()=>state.calibration.axes[3].lower===null&&commands.at(-1).action==='select');assert.equal(state.calibration.axes[3].reference,2000);await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled);assert.equal(await el('target').isDisabled(),true);
 // Beyond a saved pose: explained with a warning, manual moves stay available; reset only it.
 state.calibration.axes[3]={lower:879,upper:2176,reference:2000,reverse:false};state.samples[3].position_raw=2183;
 await motor(3).click();await until(()=>state.enabled_id===3);await page.waitForFunction(()=>document.querySelector('#fixture-reset').textContent==='Reset upper pose');
 assert.equal(await el('plus').isDisabled(),false);assert.match(await el('status').textContent(),/Move back inward freely/);await el('reset').click();await until(()=>state.calibration.axes[3].upper===null&&commands.at(-1).action==='select');
 assert.equal(state.calibration.axes[3].lower,879);await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled);
 // Continuous readback crosses zero without being mistaken for an end stop.
 state.samples[3].position_raw=4095;state.samples[3].position_continuous=-1;state.calibration.axes[3]={lower:null,upper:null,reference:null,reverse:false};
 await motor(3).click();await page.waitForFunction(()=>document.querySelector('#fixture-position').textContent.includes('-0.1'));
 await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled); assert.equal(await el('plus').isEnabled(),true);assert.equal(await el('minus').isEnabled(),true);
 // A saved multi-turn pose from another session is ignored (with a warning), not reused; manual moves continue.
 state.calibration.axes[3]={lower:-100,upper:100,coordinate_session:'old',reverse:false};state.coordinate_session='new';
 await motor(3).click();await page.waitForFunction(()=>document.querySelector('#fixture-reset').textContent==='Re-teach both poses');await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled);assert.equal(await el('plus').isDisabled(),false);assert.equal(await el('sweep').isDisabled(),true);
 state.calibration.axes[3]={lower:null,upper:null,reverse:false};await motor(3).click();await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled);
 // Stop during a delayed start acknowledgement must not resurrect heartbeat.
 delayStart=300;await page.keyboard.down('q');await until(()=>commands.at(-1).action==='motion_start');await page.keyboard.press('z');await page.keyboard.up('q');await until(()=>commands.at(-1).action==='stop');const end=commands.length;await new Promise(r=>setTimeout(r,600));assert.equal(commands.length,end);
 // Disable the knee: it is stopped first, cannot be selected, and sweep-all skips it.
 await page.keyboard.press('z');for(const k of [1,2,3])state.calibration.axes[k]={...state.calibration.axes[k],lower:k===2?3000:1000,upper:k===2?1000:3000,coordinate_session:null};state.samples[3]={position_raw:2000,voltage_v:12,temperature_c:25,current_raw:0};
 await motor(1).click();await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled);
 await el('disable').click();await until(()=>state.calibration.axes[1].disabled===true);
 assert.equal(commands.filter(c=>c.action==='set_disabled').at(-1).id,1);
 const selects=commands.filter(c=>c.action==='select').length;await motor(1).click();await page.waitForTimeout(300);
 assert.equal(commands.filter(c=>c.action==='select').length,selects,'a disabled motor is never selected');
 assert.equal(await el('plus').isDisabled(),true);
 const before2=commands.length;await el('sweep-all').click();
 await page.waitForFunction(()=>/^Swept /.test(document.querySelector('#fixture-sequence').textContent),null,{timeout:15000});
 const swept=commands.slice(before2);
 const sel=swept.filter(c=>c.action==='select');assert.deepEqual(sel.map(c=>[c.id,c.hold_others]),[[2,true]],'sweep-all selects the first enabled motor with every other enabled motor proven');
 assert.equal(swept.filter(c=>c.action==='sweep_all').length,1,'one simultaneous session');
 assert.equal(swept.some(c=>c.id===1&&c.action!=='stop'),false,'disabled knee is never driven');
 assert.match(await el('sequence').textContent(),/Skipped: test \(disabled\)/);
 // Tune: only after the operator confirms mid-travel; stops, reselects alone, then runs and shows the saved gains.
 await motor(2).click();await page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled);
 assert.equal(await el('tune').isDisabled(),true,'tuning needs the mid-travel confirmation');
 await el('tune-ok').check();const beforeTune=commands.length;await el('tune').click();
 await page.waitForFunction(()=>/Tuned gains in use: kp 1.50/.test(document.querySelector('#fixture-tune-status').textContent),null,{timeout:15000});
 const tuneCmds=commands.slice(beforeTune).map(c=>c.action);assert.deepEqual(tuneCmds.filter(a=>a!=='motion_update'),['stop','select','tune']);
 assert.equal(commands.slice(beforeTune).find(c=>c.action==='select').hold_others,false,'tuning proves and drives only this motor');
 await page.screenshot({path:'/tmp/calibration-keyboard-ui.png',fullPage:true});
 console.log('PASS: one-click motor setup; Q/A continuous intent; release during startup holds; no restart between directions; hold capture; unrestricted PWM; input key isolation; Z latching; slow sweep/pause; live speed; window switch keeps control, page close stops; reset with automatic readiness; stale start cannot revive movement; disabled motor never selected or driven; sweep-all runs enabled taught motors together; tune needs confirmation, runs alone and shows saved gains.');
}finally{await browser.close();}
