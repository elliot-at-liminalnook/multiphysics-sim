// Inspect actual canvas encoding, including clips stopped before the first timeslice.
import {chromium} from 'playwright';
import {spawn} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import assert from 'node:assert/strict';
const [bundle,preset,report]=process.argv.slice(2);assert(report);
const server=spawn(process.execPath,['web/serve-viewer.mjs',bundle,'0']);
const url=await new Promise(resolve=>server.stdout.on('data',b=>{const m=String(b).match(/http:\/\/127.0.0.1:\d+/);if(m)resolve(m[0]);}));
let browser;const results=[];
try{
  browser=await chromium.launch({headless:true,...(process.env.CHROME_EXECUTABLE?{executablePath:process.env.CHROME_EXECUTABLE}:{})});
  const page=await browser.newPage({acceptDownloads:true});
  await page.addInitScript(()=>{
    window.videoEvents=[];const Original=window.MediaRecorder;
    window.MediaRecorder=class extends Original{
      constructor(...args){super(...args);for(const name of ['start','dataavailable','stop','error'])this.addEventListener(name,e=>window.videoEvents.push({event:name,at_ms:performance.now(),size:e.data?.size,error:e.error?.message}));}
    };
  });
  await page.goto(`${url}/?preset=${preset}`);await page.locator('#overlay').waitFor({state:'hidden',timeout:60000});
  await page.locator('#display-rate').selectOption('30');
  for(const duration of [.3,2]){
    await page.locator('#reset').click();await page.locator('#overlay').waitFor({state:'hidden',timeout:60000});
    await page.evaluate(()=>window.videoEvents=[]);
    await page.locator('#video').click();await page.locator('#play').click();
    await page.waitForFunction(t=>parseFloat(document.querySelector('#sim-time').textContent)>t,duration);
    const pending=page.waitForEvent('download',{timeout:10000}).catch(e=>null);await page.locator('#video').click();
    const download=await pending;const bytes=download?(await readFile(await download.path())).length:0;
    results.push({duration_s:duration,bytes,events:await page.evaluate(()=>window.videoEvents),button:await page.locator('#video').textContent()});
  }
}finally{await browser?.close();server.kill();await writeFile(report,JSON.stringify({passed:results.length===2&&results.every(r=>r.bytes>1000),results},null,2)+'\n');}
console.log(results);assert(results.every(r=>r.bytes>1000));
