// Production-worker/native parity for caller-supplied progress task captures.
// Node transports records and checks results; all dynamics/rewards run in Rust.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {chromium} from 'playwright';

const [bundle, reportPath, ...captures] = process.argv.slice(2);
assert(bundle && reportPath && captures.length, 'usage: progress.mjs bundle report.json native-capture.json ...');
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
  for (const path of captures) {
    const native = JSON.parse(fs.readFileSync(path));
    assert(native.requested_steps_completed && native.error === null);
    assert(native.task.progress, 'capture must use the shared progress task');
    const result = await page.evaluate(async ({record, task, boundaries}) => {
      const worker = new Worker('/worker.js', {type:'module'});
      const pending = new Map();
      let id = 0;
      worker.onmessage = ({data}) => {
        if (data.progress) return;
        const p = pending.get(data.id);
        if (!p) return;
        pending.delete(data.id);
        clearTimeout(p.timer);
        data.error ? p.reject(Error(data.error)) : p.resolve(data.result);
      };
      const rpc = arg => new Promise((resolve, reject) => {
        const key = ++id;
        pending.set(key, {resolve, reject, timer:setTimeout(() => reject(Error('worker timeout')), 30000)});
        worker.postMessage({...arg, id:key});
      });
      try {
        const loaded = await rpc({type:'load', scene:record.scene, config:record.config, seed:record.seed, task});
        const frames = [loaded.frame];
        // Recordings store changes, not repeated held inputs. Advance every
        // native action interval while replaying the input held at its start.
        let eventIndex = 0, action = loaded.inputs.map(input => input.initial);
        for (const boundary of boundaries) {
          while (eventIndex < record.input_events.length && record.input_events[eventIndex].at_step <= boundary)
            action = record.input_events[eventIndex++].values;
          frames.push(await rpc({type:'step', action}));
        }
        const recording = await rpc({type:'recording'});
        const replay = await rpc({type:'replay', recording});
        return {frames, recording, replay};
      } finally { worker.terminate(); }
    }, {record:native.recording, task:native.task,
      boundaries:native.transitions.slice(0,-1).map(t => t.completed_steps)});
    let numericValues = 0, maximumFraction = 0;
    const differences = [];
    function compare(a, b, path) {
      if (typeof a === 'number' && typeof b === 'number') {
        numericValues++;
        const fraction = Math.abs(a-b)/(1e-7 + 1e-8*Math.max(Math.abs(a), Math.abs(b)));
        maximumFraction = Math.max(maximumFraction, fraction);
        if (!Number.isFinite(fraction) || fraction > 1) differences.push(path);
      } else if (a && b && typeof a === 'object' && typeof b === 'object') {
        if (Array.isArray(a) !== Array.isArray(b) || (Array.isArray(a) && a.length !== b.length)) differences.push(path+'.length/type');
        for (const key of Object.keys(a)) if (key !== 'stepping_wall_s') compare(a[key], b[key], path+'.'+key);
      } else if (a !== b) differences.push(path);
    }
    assert.equal(result.frames.length, native.frames.length);
    native.frames.forEach((frame, i) => {
      compare(frame, result.frames[i], `frames.${i}`);
      compare(native.transitions[i], result.frames[i].learning, `transitions.${i}`);
    });
    compare(native.recording, result.recording.runtime, 'recording');
    compare(native.task, result.recording.task, 'task');
    compare(result.frames.at(-1), result.replay, 'replay');
    reports.push({capture:path, passed:differences.length === 0,
      transitions:native.transitions.length-1, simulated_s:native.transitions.at(-1).time_s,
      numeric_values:numericValues, maximum_tolerance_fraction:maximumFraction,
      difference_count:differences.length, differences:differences.slice(0,20)});
  }
  const report = {version:1, passed:reports.every(r => r.passed), reports,
    scope:'All native frame/transition/recording fields except stepping_wall_s, plus browser replay; tolerance 1e-7 absolute + 1e-8 relative. Sampled prefixes only; no sustained-speed or realtime qualification.'};
  fs.writeFileSync(reportPath, JSON.stringify(report, null, 2)+'\n', {flag:'wx'});
  console.log(JSON.stringify(report));
  assert(report.passed);
} finally {
  if (browser) await browser.close();
  server.kill();
}
