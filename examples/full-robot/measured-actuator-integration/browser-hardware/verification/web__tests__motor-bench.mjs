// UI integration checks. Hardware motion requires the explicit --hardware flag.
import {chromium} from 'playwright';
import fs from 'node:fs';
const [url,out,mode]=process.argv.slice(2);const hardware=mode==='--hardware';
const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
const page=await browser.newPage({viewport:{width:1280,height:1050}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
try {
 await page.goto(url);await page.waitForFunction(()=>document.querySelectorAll('#bindings tr').length===3);
 await page.locator('#leg').selectOption('+X');await page.locator('#bindings .motor').nth(0).selectOption('12');await page.locator('#bindings .motor').nth(2).selectOption('10');
 await page.locator('#amplitude').fill('0.03');await page.locator('#amplitude').dispatchEvent('input');
 await page.locator('#preview').click();await page.waitForFunction(()=>document.querySelector('#cards h3')?.textContent.includes('+X | Hip servo output · ID 12'));
 const cards=await page.locator('#cards h3').allTextContents();if(!cards[0].includes('+X | Hip')||!cards[0].includes('12')||!cards[2].includes('10'))throw Error('Explicit joint mapping was not reflected in preview');
 await page.locator('#bindings .motor').nth(1).selectOption('12');
 const rejected=page.waitForResponse(r=>r.url().endsWith('/preview')&&r.status()===400);await page.locator('#preview').click();await rejected;
 await page.locator('#bindings .motor').nth(1).selectOption('11');
 // Establish the default single-leg bench mapping before any physical run.
 await page.locator('#leg').selectOption('-Y');await page.locator('#amplitude').fill('0.03');await page.locator('#amplitude').dispatchEvent('input');
 await page.locator('#preview').click();
 let state=null;
 if(hardware){
  const launch=page.waitForResponse(r=>r.url().endsWith('/run'));await page.locator('#run').click();if((await launch).status()!==200)throw Error('Hardware launch rejected');
  await page.waitForFunction(()=>document.querySelector('#run').disabled);
  await page.waitForFunction(()=>!document.querySelector('#run').disabled&&document.querySelector('#result').textContent.includes('Saved:'),{},{timeout:30000});
  state=await page.evaluate(()=>lastState);
  if(!state.result.completed||!state.result.result.stop_verified||state.samples.length!==600)throw Error('Physical run incomplete: '+JSON.stringify(state.result));
  const ranges=state.request.bindings.map(b=>{const a=state.samples.filter(s=>s.id===b.motor_id).map(s=>s.telemetry.position_raw);return {id:b.motor_id,range_counts:Math.max(...a)-Math.min(...a)}});
  if(ranges.some(r=>r.range_counts<2))throw Error('Actual motor movement not established: '+JSON.stringify(ranges));
  state={run:state.run,result:state.result,ranges,samples:state.samples.length};
 }
 await page.screenshot({path:out+'.png',fullPage:true});
 fs.writeFileSync(out+'.json',JSON.stringify({hardware,page_errors:errors,mapping_preview:cards,state},null,2));
 if(errors.length)throw Error(errors.join('\n'));
 console.log(JSON.stringify({hardware,state,errors},null,2));
} finally {await browser.close()}
