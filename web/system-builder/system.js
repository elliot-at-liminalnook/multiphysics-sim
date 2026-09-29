// Runs system files in the browser on the shared Rust runtime (WASM).
// Simulation time is paced to the wall clock; drawing only reads results.
import init, { SystemRun } from './sim_web.js';

const COLORS = ['#4dd4bf', '#de9454', '#8caaff', '#eb6ea0'];
const $ = id => document.getElementById(id);
let catalog, run = null, series = [], raf = 0;

function interpolate(times, values, t) {
  const i = times.findIndex(x => x >= t);
  if (i === -1) return values[values.length - 1];
  if (i === 0) return values[0];
  const f = (t - times[i - 1]) / Math.max(times[i] - times[i - 1], 1e-300);
  return values[i - 1] + f * (values[i] - values[i - 1]);
}

/** Run a profile unpaced for `duration`; returns series and compute speed. */
export function simulate(systemText, profile, observe, duration) {
  const r = new SystemRun(systemText, JSON.stringify(catalog.parts), profile, JSON.stringify(observe));
  const out = observe.map(() => ({ times: [], values: [] }));
  const start = performance.now();
  // Sample every step, as the native measurement does (short transients count).
  const chunk = r.step();
  while (r.time() + 0.5 * r.step() < duration) {
    r.advance(Math.min(chunk, duration - r.time()));
    const v = r.values();
    // NaN marks a value the runtime reports unavailable (e.g. an algebraic
    // current at the instant of a switching event): no sample, not a zero.
    out.forEach((s, k) => { if (Number.isFinite(v[k])) { s.times.push(r.time()); s.values.push(v[k]); } });
  }
  const seconds = (performance.now() - start) / 1000;
  r.free();
  return { series: out, speed: duration / Math.max(seconds, 1e-9) };
}

/** Realtime profile vs detailed model, both computed in this browser. */
export function measure(systemId) {
  const entry = catalog.systems.find(s => s.id === systemId);
  const doc = JSON.parse(entry.text);
  const p = doc.realtime;
  const detailed = simulate(entry.text, 'detailed', p.observe, p.duration);
  const realtime = simulate(entry.text, 'realtime', p.observe, p.duration);
  const errors = {};
  p.observe.forEach((key, k) => {
    const d = detailed.series[k], r = realtime.series[k];
    const lo = Math.min(...d.values), hi = Math.max(...d.values);
    const range = Math.max(hi - lo, 1e-9 * Math.max(Math.abs(hi), Math.abs(lo)), 1e-12);
    let worst = 0;
    d.times.forEach((t, i) => { worst = Math.max(worst, Math.abs(interpolate(r.times, r.values, t) - d.values[i])); });
    errors[key] = worst / range;
  });
  const bound = key => (p.bounds && p.bounds[key] !== undefined) ? p.bounds[key] : p.bound;
  const within = Object.entries(errors).every(([k, e]) => e <= bound(k));
  return { system: systemId, duration: p.duration, detailed_speed: detailed.speed, realtime_speed: realtime.speed, errors, bounds: Object.fromEntries(p.observe.map(k => [k, bound(k)])), within, userAgent: navigator.userAgent };
}

function showBound() {
  const entry = catalog.systems.find(s => s.id === $('system').value);
  const p = JSON.parse(entry.text).realtime;
  if (!p) { $('bound').textContent = 'This system has no realtime profile.'; return; }
  const b = k => (p.bounds && p.bounds[k] !== undefined) ? p.bounds[k] : p.bound;
  const m = p.measured;
  $('bound').innerHTML = p.observe.map(k => `<div class="row"><span>${k}</span><span>≤ ${(100 * b(k)).toFixed(1)} %${m ? ` (native ${(100 * m.errors[k]).toFixed(2)} %)` : ''}</span></div>`).join('') +
    `<p>${p.notes || ''}</p>` + (m ? `<p>Native ${m.host}: detailed ${m.detailed_speed.toFixed(1)}×, realtime ${m.realtime_speed.toFixed(1)}× realtime.</p>` : '');
}

function draw() {
  const c = $('plot'), g = c.getContext('2d');
  g.clearRect(0, 0, c.width, c.height);
  g.strokeStyle = '#28303a'; g.lineWidth = 1;
  for (let k = 1; k < 4; k++) { g.beginPath(); g.moveTo(0, c.height * k / 4); g.lineTo(c.width, c.height * k / 4); g.stroke(); }
  series.forEach((s, k) => {
    if (s.times.length < 2) return;
    const t1 = s.times[s.times.length - 1], t0 = Math.max(s.times[0], t1 - 10);
    const lo = Math.min(...s.values), hi = Math.max(...s.values), span = Math.max(hi - lo, 1e-9);
    g.strokeStyle = COLORS[k % COLORS.length]; g.lineWidth = 2.5; g.beginPath();
    s.times.forEach((t, i) => {
      if (t < t0) return;
      const x = (t - t0) / Math.max(t1 - t0, 1e-9) * c.width, y = c.height - ((s.values[i] - lo) / span * 0.84 + 0.08) * c.height;
      i ? g.lineTo(x, y) : g.moveTo(x, y);
    });
    g.stroke();
  });
}

function start() {
  cancelAnimationFrame(raf);
  if (run) run.free();
  const entry = catalog.systems.find(s => s.id === $('system').value);
  const doc = JSON.parse(entry.text);
  const observe = doc.realtime ? doc.realtime.observe : [];
  try {
    run = new SystemRun(entry.text, JSON.stringify(catalog.parts), $('profile').value, JSON.stringify(observe));
  } catch (e) { $('status').textContent = String(e); return; }
  series = observe.map(label => ({ label, times: [], values: [] }));
  $('legend').innerHTML = observe.map((l, k) => `<span><i style="background:${COLORS[k % COLORS.length]}"></i>${l} (scaled)</span>`).join('');
  $('step').textContent = `${(run.step() * 1000).toFixed(2)} ms`;
  const wall0 = performance.now();
  let busy = 0;
  const frame = () => {
    const target = (performance.now() - wall0) / 1000;
    const before = performance.now();
    if (target > run.time()) run.advance(Math.min(target - run.time() + 1e-9, 0.25));
    busy += performance.now() - before;
    const v = run.values();
    series.forEach((s, k) => { if (Number.isFinite(v[k])) { s.times.push(run.time()); s.values.push(v[k]); } });
    $('t').textContent = `${run.time().toFixed(3)} s`;
    const wall = (performance.now() - wall0) / 1000;
    $('pace').textContent = `${(run.time() / Math.max(wall, 1e-9)).toFixed(2)}× realtime`;
    $('headroom').textContent = `${(run.time() / Math.max(busy / 1000, 1e-9)).toFixed(1)}× (simulated s per compute s)`;
    draw();
    raf = requestAnimationFrame(frame);
  };
  $('status').textContent = `Running ${entry.label} (${$('profile').value})`;
  raf = requestAnimationFrame(frame);
}

async function main() {
  await init();
  catalog = await (await fetch('data/catalog.json')).json();
  $('system').innerHTML = catalog.systems.map(s => `<option value="${s.id}">${s.label}</option>`).join('');
  $('system').onchange = () => { showBound(); start(); };
  $('profile').onchange = start;
  $('run').onclick = start;
  $('measure').onclick = () => {
    $('measured').textContent = 'Measuring…';
    setTimeout(() => {
      const m = measure($('system').value);
      $('measured').innerHTML = Object.entries(m.errors).map(([k, e]) => `<div class="row"><span>${k}</span><span class="${e > m.bounds[k] ? 'bad' : ''}">${(100 * e).toFixed(2)} %</span></div>`).join('') +
        `<p>Detailed ${m.detailed_speed.toFixed(1)}×, realtime ${m.realtime_speed.toFixed(1)}× realtime in this browser. ${m.within ? 'Within the published bound.' : 'Outside the published bound.'}</p>`;
    }, 20);
  };
  showBound();
  window.__systemRunner = { measure, simulate, ready: true };
  $('status').textContent = 'Ready: shared Rust runtime loaded.';
}
main().catch(e => { $('status').textContent = String(e); window.__systemRunner = { error: String(e) }; });
