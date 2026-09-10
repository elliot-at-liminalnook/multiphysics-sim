// Compare the same shared Rust inspection through native and production worker.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {chromium} from 'playwright';
const [bundle, reportPath, ...pairs] = process.argv.slice(2);
assert(bundle && reportPath && pairs.length && pairs.length % 2 === 0,
  'usage: robot-contract.mjs bundle report.json document.json native.json ...');
const server = spawn(process.execPath, ['web/serve-viewer.mjs', bundle, '0'], {stdio:['ignore','pipe','inherit']});
let browser;
try {
  const url = await new Promise((resolve, reject) => {
    let output = '';
    server.stdout.on('data', chunk => {
      output += chunk;
      const match = output.match(/http:\/\/127\.0\.0\.1:\d+/);
      if (match) resolve(match[0]);
    });
    server.once('error', reject);
    server.once('exit', code => reject(Error(`server exited ${code}`)));
  });
  browser = await chromium.launch({headless:true,
    ...(process.env.CHROME_PATH ? {executablePath:process.env.CHROME_PATH} : {})});
  const page = await browser.newPage();
  await page.goto(url);
  const reports = [];
  for (let i = 0; i < pairs.length; i += 2) {
    const document = JSON.parse(fs.readFileSync(pairs[i]));
    const native = JSON.parse(fs.readFileSync(pairs[i+1]));
    const actual = await page.evaluate(async document => {
      const worker = new Worker('/worker.js', {type:'module'});
      try {
        return await new Promise((resolve, reject) => {
          const timer = setTimeout(() => reject(Error('inspection timeout')), 30000);
          worker.onerror = event => { clearTimeout(timer); reject(Error(event.message)); };
          worker.onmessage = ({data}) => {
            clearTimeout(timer);
            data.error ? reject(Error(data.error)) : resolve(data.result);
          };
          worker.postMessage({id:1, type:'inspect_robot', document});
        });
      } finally { worker.terminate(); }
    }, document);
    assert.deepEqual(actual, native);
    reports.push({document:pairs[i], native:pairs[i+1], passed:true,
      entities:actual.robot.entities.length, relations:actual.robot.relations.length,
      defaulted_fields:actual.robot.defaulted_fields.length});
  }
  const report = {version:1, passed:true, reports,
    scope:'Exact full inspection JSON equality: original authoring evidence, identities, relations, default audit, properties/units and shared registry descriptions. No dynamics or policy-transfer claim.'};
  fs.writeFileSync(reportPath, JSON.stringify(report, null, 2)+'\n', {flag:'wx'});
  console.log(JSON.stringify(report));
} finally {
  if (browser) await browser.close();
  server.kill();
}
