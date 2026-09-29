// Measure, in headless Chromium, that systems built from library and
// authored parts run faster than realtime in their realtime profile and stay
// within the published error bound against the detailed model (also computed
// in the browser). Writes a report under runs/browser-realtime/.
import { chromium } from 'playwright';
import { createServer } from 'node:http';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve, extname, sep, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
const root = resolve(import.meta.dirname, '..');
const bundle = resolve(root, 'runs/interactive/system-builder');
execFileSync(process.execPath, [join(root, 'web/build-system-builder.mjs'), bundle], { stdio: 'inherit' });
const server = createServer(async (req, res) => {
  try {
    const path = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
    const file = resolve(bundle, '.' + (path === '/' ? '/index.html' : path));
    if (!file.startsWith(bundle + sep)) throw new Error('path');
    res.setHeader('Content-Type', ({ '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json', '.wasm': 'application/wasm' })[extname(file)] || 'application/octet-stream');
    res.end(await readFile(file));
  } catch { res.statusCode = 404; res.end('Not found'); }
}).listen(0, '127.0.0.1');
await new Promise(r => server.once('listening', r));
// Playwright's own Chromium when installed, else the system's Chrome.
const browser = await chromium.launch().catch(() => chromium.launch({ channel: 'chrome' }));
const page = await browser.newPage();
page.on('console', m => { if (m.type() === 'error') console.error('page:', m.text()); });
await page.goto(`http://127.0.0.1:${server.address().port}/`);
await page.waitForFunction(() => window.__systemRunner && (window.__systemRunner.ready || window.__systemRunner.error), null, { timeout: 120000 });
const error = await page.evaluate(() => window.__systemRunner.error);
assert(!error, error);
const results = [];
for (const id of ['worm-winch', 'gearmotor-winch', 'leg']) {
  const r = await page.evaluate(id => window.__systemRunner.measure(id), id);
  console.log(`${id}: detailed ${r.detailed_speed.toFixed(1)}×, realtime ${r.realtime_speed.toFixed(1)}× realtime; errors ${Object.entries(r.errors).map(([k, e]) => `${k} ${(100 * e).toFixed(2)} % (≤ ${(100 * r.bounds[k]).toFixed(1)} %)`).join(', ')}`);
  results.push(r);
}
await browser.close();
server.close();
const report = { version: 1, created: new Date().toISOString(), browser: results[0]?.userAgent, results };
await mkdir(join(root, 'runs/browser-realtime'), { recursive: true });
const out = join(root, 'runs/browser-realtime', `report-${Date.now()}.json`);
await writeFile(out, JSON.stringify(report, null, 2));
console.log(`report: ${out}`);
for (const r of results) {
  assert(r.within, `${r.system}: outside the published bound`);
  assert(r.realtime_speed >= 1, `${r.system}: realtime profile below realtime in the browser (${r.realtime_speed.toFixed(2)}×)`);
}
console.log('All systems run faster than realtime in the browser within their published error bounds.');
