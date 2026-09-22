// Explicit hardware stop/disconnect verification at 3% reference amplitude.
import {chromium} from 'playwright';import fs from 'node:fs';
const [url,out]=process.argv.slice(2);const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});const results=[];
try{
 for(const mode of ['button','disconnect']){
  let page=await browser.newPage();await page.goto(url);await page.waitForFunction(()=>document.querySelectorAll('#bindings tr').length===3);
  await page.locator('#amplitude').fill('0.03');await page.locator('#amplitude').dispatchEvent('input');
  const launch=page.waitForResponse(r=>r.url().endsWith('/run'));await page.locator('#run').click();if((await launch).status()!==200)throw Error('Start rejected');
  await page.waitForFunction(()=>lastState?.active&&lastState.samples.length>=30,{},{timeout:20000});
  const run=await page.evaluate(()=>lastState.run);
  if(mode==='button')await page.locator('#stop').click();
  else {
   // Keep a separate observing tab open: it must not renew the owner's lease.
   const observer=await browser.newPage();await observer.goto(url);await observer.waitForFunction(()=>lastState?.active);
   await page.close();page=observer;
  }
  await page.waitForFunction(run=>lastState?.run===run&&!lastState.active&&lastState.result?.completed===false,run,{timeout:15000});
  const s=await page.evaluate(()=>lastState);
  if(!s.result.result.stop_verified||s.samples.length===0||s.samples.length>=600)throw Error('Did not establish bounded early stop: '+JSON.stringify(s.result));
  results.push({mode,run,samples:s.samples.length,result:s.result,stop_reason:fs.readFileSync(run+'/capture/STOP','utf8')});
  await page.close();
 }
 fs.writeFileSync(out,JSON.stringify(results,null,2));console.log(results.map(r=>({mode:r.mode,samples:r.samples,stopped:r.result.result.stop_verified,reason:r.stop_reason})));
}finally{await browser.close()}
