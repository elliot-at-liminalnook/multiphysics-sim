// Preserve completed pre-joint-optimizer experiments before rebuilding binaries.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
const dir = 'examples/full-robot/contact-planning';
const read = p => JSON.parse(fs.readFileSync(p));
const hash = b => crypto.createHash('sha256').update(b).digest('hex');
const identity = p => { const b = fs.readFileSync(p); return { path: p, bytes: b.length, sha256: hash(b) }; };
const cases = ['xhip-contact-damping1', 'xhip-contact-damping2', 'return-x25-v230',
  'return-x25-v250', 'return-all25-v230', 'return-x15-tight-v230', 'return-x25-lift-v250'];
const rows = cases.map(name => {
  const s = read(`${dir}/${name}.summary.json`), c = read(`${dir}/${name}-clearance.summary.json`);
  assert(s.completed && s.frames === 401 && c.maximum_command_error_rad === 0);
  assert.equal(s.capture_sha256, identity(`runs/contact-planning/${name}.native.json`).sha256);
  return { name, speed_m_s: s.segments.map(v => v.speed_along_heading_m_s),
    slip_ratio: s.maximum_slip_ratio, control_passed: s.passed_control_checks,
    contact_quality_passed: s.passed_contact_quality,
    lifts: [c.passed_foot_clearances, c.planned_foot_clearances],
    geometry: c.inter_link_geometry_audit,
    inputs: [identity(`${dir}/${name}.summary.json`), identity(`${dir}/${name}-clearance.summary.json`)] };
});
fs.writeFileSync(`${dir}/smooth-return-summary.json`, JSON.stringify({ cases: rows,
  scope: 'Completed manually selected diagnostics, retained as evidence and warm starts. No fully qualified fast gait. Superseded by joint force/motion/timing optimization; no physical speed maximum or browser promotion.' }, null, 2) + '\n');
const prefixes = [...cases, 'return-x25-base', 'return-all25-base', 'return-x15-tight-base', 'return-x25-lift-base'];
const files = fs.readdirSync('runs/contact-planning').filter(n => prefixes.some(p => n.startsWith(p + '.')))
  .map(n => `runs/contact-planning/${n}`).sort();
const base = read(`${dir}/path-and-hold-evidence-index.json`);
const bins = base.binaries.map(b => identity(b.path));
const overlays = ['crates/sim-domain-control/src/contact_phase.rs', 'crates/sim-domain-control/src/smooth_return.rs',
  'crates/sim-domain-control/src/elements.rs', 'crates/sim-domain-control/src/lib.rs'];
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'smooth-return-evidence-'));
const archive = path.join(tmp, 'evidence.tar.gz');
const filelist = path.join(tmp, 'files.txt');
fs.writeFileSync(filelist, files.join('\n') + '\n');
execFileSync('tar', ['-czf', archive, '-T', filelist]);
const bytes = fs.readFileSync(archive), parts = [];
for(let i = 0, offset = 0; offset < bytes.length; i++, offset += 45 * 1024 * 1024) {
  const p = `${dir}/smooth-return-evidence.part-${String(i).padStart(2, '0')}`;
  fs.writeFileSync(p, bytes.subarray(offset, offset + 45 * 1024 * 1024), { flag: 'wx' });
  parts.push(identity(p));
}
const extracted = path.join(tmp, 'extracted'); fs.mkdirSync(extracted);
execFileSync('tar', ['-xzf', archive, '-C', extracted]);
const ids = files.map(identity);
for (const f of ids) assert.equal(identity(path.join(extracted, f.path)).sha256, f.sha256);
assert.equal(hash(Buffer.concat(parts.map(p => fs.readFileSync(p.path)))), hash(bytes));
fs.writeFileSync(`${dir}/smooth-return-evidence-index.json`, JSON.stringify({
  base_commit: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
  base: identity(`${dir}/path-and-hold-evidence-index.json`),
  archive_bytes: bytes.length, archive_sha256: hash(bytes), parts, files: ids,
  binaries: bins, compiled_source_overlays: overlays.map(identity), cad: base.cad,
  summary: identity(`${dir}/smooth-return-summary.json`),
  restore: 'Restore the base archive chain, concatenate these parts in listed order into a tar.gz, then extract at repository root. Verify all file hashes.',
  verification: `Joined archive digest and all ${ids.length} extracted files matched original bytes.`,
  scope: 'Binary identities captured before rebuilding for the joint-force implementation. Native build source is base_commit plus the four listed shared-control overlays; controller inputs and raw reports are archived. Rust tests and example recipes are separate versioned artifacts.'
}, null, 2) + '\n');
console.log(JSON.stringify({ cases: rows.length, raw_files: ids.length, archive_bytes: bytes.length, parts: parts.length, verified: true }));
