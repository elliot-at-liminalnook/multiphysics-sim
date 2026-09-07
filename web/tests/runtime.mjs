// Real-browser runtime gate. Build sim-web and generate wasm-bindgen web bindings first.
// --recording=PATH uses recorded actions/seed with a 30 s diagnostic step budget.
// Default pendulum checks retain their motion assertion and 5 s step budget.
// --require-sparse --audit requires an exercised island of at least 256 unknowns.
// --require-guarded-search --audit requires an observed guarded backtracking stop.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve, sep, extname } from 'node:path';
import { createRequire } from 'node:module';
import { isDeepStrictEqual } from 'node:util';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const [directory, scenePath, nativePath, reportPath, ...flags] = process.argv.slice(2);
assert(directory && scenePath && nativePath, 'usage: runtime.mjs web-directory scene.json native.frame.json [report.json] [--require-contacts] [--audit]');
assert(flags.every(f => ['--require-contacts', '--audit', '--require-sparse', '--require-guarded-search'].includes(f) || f.startsWith('--recording=') || f.startsWith('--impulses=')), 'unknown browser gate');
const contactGate = flags.includes('--require-contacts');
const audit = flags.includes('--audit');
const sparseGate = flags.includes('--require-sparse');
const guardedSearchGate = flags.includes('--require-guarded-search');
assert(!sparseGate || audit, '--require-sparse needs --audit');
assert(!guardedSearchGate || audit, '--require-guarded-search needs --audit');
const impulseFlags = flags.filter(f => f.startsWith('--impulses='));
assert(impulseFlags.length <= 1 && (!impulseFlags.length || audit), '--impulses requires --audit and one reference');
const impulseReference = impulseFlags.length ? JSON.parse(await readFile(impulseFlags[0].slice('--impulses='.length))).windows[0].impulses : null;
const root = resolve(directory);
const scene = JSON.parse(await readFile(scenePath));
const expected = JSON.parse(await readFile(nativePath));
const recordingFlags = flags.filter(f => f.startsWith('--recording='));
assert(recordingFlags.length <= 1, 'duplicate recording flag');
const suppliedRecording = recordingFlags.length ? JSON.parse(await readFile(recordingFlags[0].slice('--recording='.length))) : null;
if (suppliedRecording) {
  assert.equal(suppliedRecording.version, 1);
  assert(isDeepStrictEqual(suppliedRecording.scene, scene),
    'recording scene must match supplied scene; use the canonical scene embedded in the Rust recording');
  assert(suppliedRecording.actions.length > 0, 'recording must contain actions');
}
const actions = suppliedRecording?.actions ?? Array.from({length:20}, () => [0.2]);
const seed = suppliedRecording?.seed ?? 0;
const server = createServer(async (req, res) => {
  try {
    if (req.url === '/') { res.setHeader('Content-Type', 'text/html'); res.end('<!doctype html><title>Rust runtime acceptance</title>'); return; }
    const p = resolve(root, '.' + new URL(req.url, 'http://localhost').pathname);
    if (!p.startsWith(root + sep)) throw new Error('path');
    res.setHeader('Content-Type', ({ '.wasm': 'application/wasm', '.js': 'text/javascript' })[extname(p)] || 'application/octet-stream');
    res.end(await readFile(p));
  } catch { res.statusCode = 404; res.end(); }
});
await new Promise(done => server.listen(0, '127.0.0.1', done));
let browser;
try {
  browser = await chromium.launch({ headless: true, ...(process.env.CHROME_EXECUTABLE ? { executablePath: process.env.CHROME_EXECUTABLE } : {}) });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  const result = await page.evaluate(async ({scene,audit,actions,seed,recorded,impulseGate}) => {
    const worker = new Worker('/worker.js', { type: 'module' });
    const pending = new Map(); let sequence = 0;
    worker.onmessage = ({ data }) => {
      const p = pending.get(data.id); if (!p) return;
      clearTimeout(p.timer); pending.delete(data.id);
      if (data.error) p.reject(new Error(data.error)); else p.resolve(data.result);
    };
    worker.onerror = e => { for (const p of pending.values()) { clearTimeout(p.timer); p.reject(new Error(e.message)); } pending.clear(); };
    const rpc = data => new Promise((resolve, reject) => {
      const id = sequence++;
      const timer = setTimeout(() => { pending.delete(id); reject(new Error('worker request exceeded 30 s')); }, 30000);
      pending.set(id, { resolve, reject, timer }); worker.postMessage({ ...data, id });
    });
    const started = performance.now();
    const initial = await rpc({ type: 'load', scene, seed });
    if (audit) await rpc({ type: 'set_attempt_audit_limit', limit: 512 });
    const loadMs = performance.now() - started;
    let rejected = false;
    const invalidAction = recorded ? Array(initial.inputs.length + 1).fill(0) : [2];
    try { await rpc({ type: 'step', action: invalidAction }); } catch { rejected = true; }
    const afterInvalid = await rpc({ type: 'frame' });
    let heartbeat = 0; const timer = setInterval(() => heartbeat++, 10);
    const stepStarted = performance.now();
    let final; let firstWindowImpulse = null;
    let maximumContactCount = initial.frame.contacts.length;
    const frames = [initial.frame];
    for (const action of actions) {
      final = await rpc({ type: 'step', action });
      if (impulseGate && frames.length === 1) {
        firstWindowImpulse = await rpc({ type: 'contact_impulse_report', start: initial.frame.time_s, end: final.time_s });
      }
      frames.push(final);
      maximumContactCount = Math.max(maximumContactCount, final.contacts.length);
    }
    const stepMs = performance.now() - stepStarted;
    clearInterval(timer);
    const attempts = audit ? await rpc({ type: 'implicit_attempt_report' }) : null;
    const recording = await rpc({ type: 'recording' });
    const replay = await rpc({ type: 'replay', recording });
    const reset = await rpc({ type: 'reset', seed });
    worker.terminate();
    return { initial, rejected, afterInvalid, final, frames, attempts, replay, reset, loadMs, stepMs, heartbeat, maximumContactCount, recordedSteps: recording.actions.length, firstWindowImpulse };
  }, {scene,audit,actions,seed,recorded:!!suppliedRecording,impulseGate:!!impulseReference});
  // Preserve evidence even if a comparison below rejects the run.
  if (reportPath) await writeFile(reportPath + '.frames.json', JSON.stringify({ native: expected, browser: result }, null, 2) + '\n');
  let maximumDifference = 0;
  function compare(a, b, path = '') {
    assert.equal(typeof a, typeof b, path);
    if (typeof a === 'number') {
      assert(Number.isFinite(a) && Number.isFinite(b), path);
      maximumDifference = Math.max(maximumDifference, Math.abs(a-b));
      assert(Math.abs(a-b) <= 1e-7, `${path}: native ${a}, browser ${b}`);
    } else if (a && typeof a === 'object') {
      assert.deepEqual(Object.keys(a).sort(), Object.keys(b).sort(), path);
      for (const k of Object.keys(a)) compare(a[k], b[k], `${path}.${k}`);
    } else assert.equal(a, b, path);
  }
  if (Array.isArray(expected)) {
    assert.equal(expected.length, result.frames.length, 'native/browser reporting schedules');
    for (let i = 0; i < expected.length; i++) compare(expected[i], result.frames[i], `frame[${i}]`);
  } else compare(expected, result.final);
  if (impulseReference) compare(impulseReference, result.firstWindowImpulse, "firstWindowImpulse");
  assert.deepEqual(result.replay, result.final);
  assert.equal(result.reset.time_s, 0);
  assert.equal(result.afterInvalid.time_s, 0);
  assert(result.rejected);
  // The contact fixture can be mechanically obstructed before its command.
  if (!suppliedRecording) assert(result.final.joint_positions[0] > (contactGate ? 0.01 : 0.1));
  assert.equal(result.recordedSteps, actions.length);
  assert(result.loadMs < 20000, `load budget exceeded: ${result.loadMs} ms`);
  const stepBudgetMs = suppliedRecording ? 30000 : 5000;
  assert(result.stepMs < stepBudgetMs, `simulation budget ${stepBudgetMs} ms exceeded: ${result.stepMs} ms`);
  assert(result.heartbeat > 0, 'main thread did not respond while physics advanced');
  if (audit) {
    assert(result.attempts.islands.some(i => i.attempts.length > 0), 'audit did not capture any solves');
    for (const island of result.attempts.islands) {
      assert.equal(island.attempt_limit, 512);
      assert(island.attempts.length <= 512);
      assert.equal(island.capacity_reached, island.attempts.length === 512);
      for (const attempt of island.attempts) {
        assert.equal(typeof attempt.solve.committed, 'boolean', 'new captures must declare commit status');
        assert(!attempt.solve.committed || attempt.solve.solve_succeeded, 'a failed solve cannot be committed');
      }
    }
  }
  if (audit) assert(result.attempts.islands.some(i => i.attempts.some(a => a.solve.committed)), 'audit captured no committed substeps');
  if (sparseGate) assert(result.attempts.islands.some(i => i.coordinates.length >= 256 && i.attempts.length > 0),
    'fixture did not exercise an island above the default sparse factorization threshold');
  const capturedGuardedSearchStops = audit ? result.attempts.islands.reduce((total, island) => total +
    island.attempts.reduce((n, attempt) => n + attempt.solve.newton.iterations.filter(it => it.line_search?.bracketed).length, 0), 0) : 0;
  if (guardedSearchGate) assert(capturedGuardedSearchStops > 0, 'fixture did not exercise guarded backtracking');
  if (contactGate) assert(result.maximumContactCount > 0, 'contact fixture never touched');
  const report = { passed: true, browser: await browser.version(), maximumNativeWasmDifference: maximumDifference,
    loadMs: result.loadMs, simulationWallMs: result.stepMs,
    ...(!suppliedRecording ? {simulation400msWallMs: result.stepMs} : {}), mainThreadHeartbeats: result.heartbeat,
    simulatedSeconds: result.final.time_s, jointAngleRad: result.final.joint_positions[0], recordingReplayExact: true,
    maximumContactCount: result.maximumContactCount,
    recordedInputMode: !!suppliedRecording, impulseGate: !!impulseReference, sparseGate, guardedSearchGate, capturedGuardedSearchStops, stepBudgetMs,
    comparedFrames: Array.isArray(expected) ? expected.length : 1 };
  if (reportPath) await writeFile(reportPath, JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
} finally {
  await browser?.close();
  await new Promise(done => server.close(done));
}
