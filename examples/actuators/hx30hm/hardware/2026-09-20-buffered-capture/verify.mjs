// Reconcile independent RTL observations with the shared Rust decoder.
// Run after run.mjs and building review_fpga_batch. No hardware access.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../../../../..');
const read = p => JSON.parse(fs.readFileSync(p, 'utf8'));
function checked(command, args, log) {
  const r = spawnSync(command, args, {cwd: root, encoding: 'utf8'});
  if (log) fs.writeFileSync(path.join(here, 'results', log), r.stdout + r.stderr);
  assert.equal(r.status, 0, r.stderr);
}
checked(path.join(os.homedir(), '.cargo/bin/cargo'), ['test', '--locked', '-p',
  'sim-runtime', '--test', 'fpga_batch', '--test', 'fpga_events', '--test', 'fpga_upload'], 'rust-tests.log');
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'hx-frame-unit-'));
for (const [mode, log] of [[0, 'overflow-test.log'], [1, 'partial-test.log']]) {
  const executable = path.join(temp, `${mode}.vvp`);
  checked('iverilog', ['-g2012', '-s', 'frame_log_overflow_tb', '-P',
    `frame_log_overflow_tb.MODE=${mode}`, '-o', executable,
    path.join(here, 'firmware/tb/frame_log_overflow_tb.v'),
    path.join(here, 'firmware/src/experiment_frame_log.v')]);
  checked('vvp', [executable], log);
}
const cases = read(path.join(here, 'results/summary.json'));
assert.equal(cases.length, 10);
assert.equal(new Set(cases.map(c => c.name)).size, 10);
const counts = [];
for (const c of cases) {
  const dir = path.join(here, 'results', c.name);
  const decoded = path.join(dir, 'review.json');
  const result = spawnSync(path.join(root, 'target/debug/examples/review_fpga_batch'),
    [path.join(dir, 'uart.hex'), decoded], {encoding: 'utf8'});
  assert.equal(result.status, 0, result.stderr);
  const review = read(decoded);
  const direct = read(path.join(dir, 'events.json'));
  // Internal transaction kinds are zero-based; A3/A4 event kinds include START.
  const kinds = {Telemetry: 0, Control: 1, Audit: 2};
  const expanded = review.events.filter(e => e.kind in kinds).map(e => ({
    frame: e.frame, kind: kinds[e.kind], id: e.motor_id,
    request: e.request_ticks, completion: e.completion_ticks,
  }));
  assert.deepEqual(expanded, direct, c.name);
  if (c.scenario === 0) {
    assert.equal(review.terminal_result, 0);
    assert.equal(review.frames.length, 25);
    assert.equal(review.events.length, 177);
    for (const f of review.frames) {
      assert.equal(f.raw.length, 99);
      assert.equal(f.events.length, 7);
      assert.equal(f.partial, false);
      assert.equal(f.dropped_raw_bytes, 0);
    }
    const batches = review.transport_packets.filter(p => p[6] === 0xa4);
    assert.equal(batches.length, 25);
    assert(batches.every(p => p.length === 226));
  } else assert.notEqual(review.terminal_result, 0);
  counts.push({name: c.name, frames: review.frames.length,
    events: direct.length, terminal_result: review.terminal_result});
}

const gait = read(path.join(here, 'gait-rate-study/summary.json'));
assert.equal(gait.length, 54);
const timestep = gait.filter(r => 'timestep_rms_delta_degrees' in r);
assert.equal(timestep.length, 9);
assert(timestep.every(r => r.timestep_rms_delta_degrees <= r.timestep_tolerance_degrees));
const synthesisLog = fs.readFileSync(path.join(here, 'results/full-image-synthesis.log'), 'utf8');
assert(synthesisLog.includes('End of script.') && synthesisLog.includes('Found and reported 0 problems.'));
assert(!/implicitly declared|used but has no driver/.test(synthesisLog));
const finalStats = synthesisLog.slice(synthesisLog.lastIndexOf('=== top ==='));
const countCell = name => Number(finalStats.match(new RegExp(`^\\s+(\\d+)\\s+${name}\\s*$`, 'm'))?.[1] ?? 0);
const resources = {luts: [1, 2, 3, 4].reduce((sum, n) => sum + countCell(`LUT${n}`), 0),
  flip_flops: countCell('DFFRE') + countCell('DFFSE'),
  memory_primitives: countCell('DPX9B') + countCell('SDPX9B'),
  device_lut_capacity: 23040, placed_and_routed: false, hardware_timing_verified: false};
assert(resources.luts > 0 && resources.luts <= resources.device_lut_capacity,
  'Candidate must fit the FPGA logic budget before qualification');
const rustLog = fs.readFileSync(path.join(here, 'results/rust-tests.log'), 'utf8');
for (const n of [5, 8, 10]) assert(rustLog.includes(`test result: ok. ${n} passed; 0 failed`));
for (const name of ['overflow-test.log', 'partial-test.log']) {
  assert(fs.readFileSync(path.join(here, 'results', name), 'utf8').includes('PASS'));
}

const hashes = {};
function hashFile(p) {
  hashes[path.relative(root, p)] = createHash('sha256').update(fs.readFileSync(p)).digest('hex');
}
function walk(dir) {
  for (const e of fs.readdirSync(dir, {withFileTypes: true})) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p);
    else if (e.isFile() && e.name !== 'verification.json') hashFile(p);
  }
}
// Compare the immutable build input with the retained final firmware. Uploaded
// fixture packets/CRC in the temporary run vary by test cadence deliberately.
const work = fs.readFileSync(path.join(here, 'results/workspace.txt'), 'utf8').trim();
for (const name of fs.readdirSync(path.join(here, 'firmware/src'))) {
  assert.deepEqual(fs.readFileSync(path.join(here, 'firmware/src', name)),
    fs.readFileSync(path.join(work, 'src', name)), `Build/source mismatch: ${name}`);
}
walk(here);
for (const rel of ['src/controller_refinement', 'tests/fpga_batch.rs',
  'tests/fpga_events.rs', 'tests/fpga_upload.rs', 'examples/compare_control_cadence.rs',
  'examples/review_fpga_batch.rs']) {
  const p = path.join(root, 'crates/sim-runtime', rel);
  if (fs.statSync(p).isDirectory()) walk(p); else hashFile(p);
}
for (const rel of ['Cargo.lock', '.github/workflows/simulation.yml']) hashFile(path.join(root, rel));
for (const id of [10, 11, 12]) for (const pattern of ['gait', 'reversal', 'full-gait']) {
  hashFile(path.join(root, 'examples/full-robot/measured-actuator-integration/controller-tracking-full-drive-simulation',
    `id${id}-${pattern}-nominal-selected-1000.json`));
}
const versions = {};
for (const [name, command, args] of [['node', process.execPath, ['--version']],
  ['verilator', 'verilator', ['--version']], ['iverilog', 'iverilog', ['-V']],
  ['rustc', '/Users/elliot/.cargo/bin/rustc', ['--version']],
  ['yosys', '/Users/elliot/.local/opt/oss-cad-suite/bin/yosys', ['-V']]]) {
  const r = spawnSync(command, args, {encoding: 'utf8'});
  assert.equal(r.status, 0, name);
  versions[name] = r.stdout.trim().split('\n')[0];
}
const verification = {simulation_only: true, hardware_accessed: false,
  cases: counts, rust_tests_passed: 23, logger_unit_modes_passed: 2,
  physics_cases: gait.length, timestep_comparisons: timestep.length,
  max_timestep_rms_delta_degrees: Math.max(...timestep.map(r => r.timestep_rms_delta_degrees)),
  build_source_matches: true, synthesis_resources: resources, versions, sha256: hashes};
fs.writeFileSync(path.join(here, 'results/verification.json'), JSON.stringify(verification, null, 2));
console.log('PASS: 10 UART cases reconciled, 23 Rust tests, 2 logger modes, 54 physics cases, 9 timestep comparisons; source/artifact hashes retained.');
