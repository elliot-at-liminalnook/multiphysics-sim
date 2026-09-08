// Real browser, worker, recipe downloads and canvas encoding.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import {resolve, dirname} from 'node:path';
import {chromium} from 'playwright';
import {createHash} from 'node:crypto';
const directory = resolve(process.argv[2] || 'runs/interactive/whole-swing/viewer');
const reportPath = resolve(process.argv[3] || 'runs/interactive/whole-swing/leaderboard-report.json');
const data = JSON.parse(await readFile(resolve(directory, 'leaderboard.json')));
const source = async path => ({path, sha256: createHash('sha256').update(await readFile(path)).digest('hex')});
const sources = await Promise.all(['leaderboard.json', 'build-manifest.json'].map(p => source(resolve(directory, p))));
sources.push(await source(import.meta.filename));
assert(data.entries.length >= 3);
const server = spawn(process.execPath, ['web/serve-viewer.mjs', directory, '0']);
const url = await new Promise((done, fail) => { server.once('error', fail); server.stdout.on('data', b => { const m = String(b).match(/http:\/\/127.0.0.1:\d+/); if (m) done(m[0]); }); });
let browser; const checks = [], errors = [];
try {
  browser = await chromium.launch({headless: true, ...(process.env.CHROME_EXECUTABLE ? {executablePath: process.env.CHROME_EXECUTABLE} : {})});
  const page = await browser.newPage({viewport: {width: 1440, height: 950}, acceptDownloads: true});
  await page.addInitScript(() => {
    window.videoEvents = [];
    const Original = window.MediaRecorder;
    if (Original) window.MediaRecorder = class extends Original {
      constructor(...args) {
        super(...args);
        for (const name of ['start', 'dataavailable', 'stop', 'error']) this.addEventListener(name, e =>
          window.videoEvents.push({event: name, at_ms: performance.now(), size: e.data?.size, error: e.error?.message}));
      }
    };
  });
  page.on('pageerror', e => errors.push(e.message));
  const ready = () => page.locator('#overlay').waitFor({state: 'hidden', timeout: 60000});
  const open = () => page.locator('#open-leaderboard').click();
  const row = id => page.locator(`[data-controller="${id}"]`);
  await page.goto(url); await ready();
  const displayControl = await page.locator('#display-rate').count() > 0;
  if (displayControl) {
    assert.equal(await page.locator('#display-rate').inputValue(), '0');
    await page.locator('#display-rate').selectOption('30');
  }
  await open();
  assert.equal(await page.locator('#leaderboard-rows tr').count(), data.entries.length);
  assert.match(await page.locator('#leaderboard-summary').textContent(), /0 meet every declared gate/);
  await page.locator('#controller-status').selectOption('validated');
  assert.equal(await page.locator('#leaderboard-rows tr').count(), 0); assert(await page.locator('#leaderboard-empty').isVisible());
  await page.locator('#controller-status').selectOption('all');
  await page.locator('#controller-search').fill('Faster student'); assert.equal(await page.locator('#leaderboard-rows tr').count(), 1);
  await page.locator('#controller-search').fill('');
  for (const e of data.entries.slice(0, 2)) await row(e.id).locator('input').check();
  await page.locator('#compare-controllers').click(); assert.equal(await page.locator('#controller-comparison article').count(), 2);
  assert.match(await page.locator('#controller-comparison').textContent(), /Different comparison groups/);
  checks.push('failed and missing gates remain unranked; search, status and comparison work');
  for (const entry of data.entries) {
    await row(entry.id).getByRole('button', {name: 'Load and run', exact: true}).click(); await ready();
    await page.waitForFunction(() => parseFloat(document.querySelector('#sim-time').textContent) > 0);
    await page.locator('#play').click(); await page.waitForFunction(() => document.querySelector('#execution-state').textContent === 'Paused');
    assert.equal(await page.locator('#preset').inputValue(), entry.preset_id);
    const pending = page.waitForEvent('download'); await page.locator('#download').click();
    const download = await pending, recording = JSON.parse(await readFile(await download.path()));
    const recipe = JSON.parse(await readFile(resolve(directory, `data/${entry.preset_id}.json`)));
    assert.deepEqual(recording.runtime.scene.controller, recipe.scene.controller);
    assert.deepEqual(recording.runtime.config.policy.neural_residual, recipe.config.policy.neural_residual);
    assert.equal(recording.runtime.seed, entry.load.seed);
    assert.deepEqual(recording.runtime.input_events[0].values, entry.replay.input_events[0].values);
    await open();
  }
  checks.push('every Load and run executes its pinned controller, seed and tested initial action through WASM');
  const shortest = data.entries.find(e => e.id === 'integral-teacher-steering') ?? data.entries.find(e => e.id === 'fast-distilled-steering') ?? data.entries.find(e => e.id === 'exact-base-steering') ?? data.entries.find(e => e.id === 'portable-tangent-steering') ?? data.entries.find(e => e.id === 'tangent-steering-short') ?? data.entries.find(e => e.id === 'secant-steering-short') ?? data.entries.find(e => e.id === 'faster-steering-short') ?? [...data.entries].sort((a, b) => a.replay.completed_steps - b.replay.completed_steps)[0];
  await row(shortest.id).getByRole('button', {name: 'Replay tested inputs', exact: true}).click();
  await page.waitForFunction(end => parseFloat(document.querySelector('#sim-time').textContent) >= end - 1e-8, shortest.metrics.simulated_s, {timeout: 180000}); await ready();
  assert.equal(parseFloat(await page.locator('#sim-time').textContent()), shortest.metrics.simulated_s);
  checks.push('tested sparse input sequence re-executes its full duration in Rust');
  await page.locator('#reset').click(); await ready();
  await page.locator('#video').click(); await page.locator('#play').click();
  await page.waitForFunction(() => parseFloat(document.querySelector('#sim-time').textContent) > .3);
  const videoPending = page.waitForEvent('download'); await page.locator('#video').click();
  let video;
  try { video = await videoPending; }
  catch (error) {
    await writeFile(reportPath.replace(/\.json$/, '.video-error.json'), JSON.stringify({error: error.message, events: await page.evaluate(() => window.videoEvents), button: await page.locator('#video').textContent(), sources}, null, 2));
    throw error;
  }
  assert.match(video.suggestedFilename(), /\.webm$/); assert((await readFile(await video.path())).length > 1000);
  await open(); await page.screenshot({path: reportPath.replace(/\.json$/, '.desktop.png')});
  await page.setViewportSize({width: 390, height: 844});
  await page.screenshot({path: reportPath.replace(/\.json$/, '.mobile.png')});
  assert(await page.evaluate(() => document.querySelector('#leaderboard-dialog').getBoundingClientRect().right <= innerWidth));
  checks.push('canvas video downloads real WebM bytes; dialog fits desktop and narrow viewports');
  await page.locator('#close-leaderboard').click();
  if (displayControl) {
    assert.equal(await page.locator('#display-rate').inputValue(), '30');
    const bounds = await page.locator('#display-rate').boundingBox();
    assert(bounds.x >= 0 && bounds.x + bounds.width <= 390);
    await page.locator('#viewport').screenshot({path: reportPath.replace(/\.json$/, '.viewer-mobile.png')});
    checks.push('optional 30 fps display persists through exact loads, replay and video; control fits the narrow viewport');
  }
  await page.route(`**/data/${shortest.preset_id}.json`, async route => { const response = await route.fetch(); await route.fulfill({response, body: (await response.text()) + ' '}); });
  await page.locator('#preset').selectOption(shortest.preset_id);
  await page.waitForFunction(() => document.querySelector('#status').textContent.includes('integrity mismatch'));
  assert(await page.locator('#play').isDisabled());
  checks.push('mutated recipe bytes are rejected before physics can run');
  assert.deepEqual(errors, []);
  await mkdir(dirname(reportPath), {recursive: true}); await writeFile(reportPath, JSON.stringify({version: 1, passed: true, browser: browser.version(),
    recipes: data.entries.map(e => ({id: e.id, asset_sha256: e.load.asset_sha256})), sources, checks}, null, 2));
  console.log(JSON.stringify({passed: true, checks}));
} finally { await browser?.close(); server.kill(); }
