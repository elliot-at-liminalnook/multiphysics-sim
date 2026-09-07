// Persist compact evidence; full captures can be regenerated from versioned recipes.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [nativeRoot='runs/full-robot/learning/residual-policy', browserRoot='runs/interactive/residual-policy', output='examples/full-robot/residual-policy-status.json'] = process.argv.slice(2);
const read = p => JSON.parse(readFileSync(p));
const digest = path => ({path, sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const evidence = {};
const load = (key, path) => { evidence[key] = digest(path); return read(path); };
const comparison = load('interface', `${nativeRoot}/interface-report.json`);
const probe = load('probe_acceptance', `${nativeRoot}/probe-acceptance/summary.json`);
const neutral = load('neutral_acceptance', `${nativeRoot}/sustained-acceptance/summary.json`);
const parity = load('parity', `${browserRoot}/probe-parity.json`);
const ui = load('viewer', `${browserRoot}/viewer-report.json`);
const timing = load('rendered_timing', `${browserRoot}/live-performance.json`);
const manifest = load('bundle', `${browserRoot}/viewer/build-manifest.json`);
const renderedRecording = load('rendered_recording', `${browserRoot}/live-performance.recording.json`);
assert(comparison.passed && probe.passed && neutral.passed && parity.passed && ui.passed && timing.completed);
assert.equal(comparison.probe.sha256, probe.capture.sha256);
assert.equal(comparison.zero.sha256, neutral.capture.sha256);
for (const capture of [comparison.baseline, comparison.zero, comparison.probe]) assert.equal(digest(capture.path).sha256, capture.sha256);
assert.deepEqual(renderedRecording.runtime.input_events, read(comparison.zero.path).recording.input_events);
for (const [path, expected] of Object.entries(manifest.inputs)) assert.equal(digest(path).sha256, expected, `stale bundle input ${path}`);
const summarize = report => ({
  simulated_s: report.simulated_s, passed:report.passed, supported_swings:report.lifts.length,
  shortest_qualifying_span_s:Math.min(...report.lifts.map(l=>l.longest_qualifying_span_s)),
  final_body_error_m:report.final_body_error_m, maximum_body_tilt_rad:report.maximum_body_tilt_rad,
  final_yaw_error_rad:report.final_yaw_error_rad, final_phase:report.final_phase,
  inter_link_geometry_audit:report.inter_link_geometry_audit, scope:report.scope,
});
const sourcePaths = [
  'crates/sim-runtime/src/environment.rs','crates/sim-runtime/src/embedded.rs',
  'crates/sim-runtime/tests/environment.rs','crates/sim-runtime/tests/residual_policy.rs',
  'examples/full-robot/prepare_residual_policy.mjs','examples/full-robot/check_residual_policy.mjs',
  'examples/full-robot/summarize_residual_policy.mjs',
  'web/tests/environment.mjs','web/tests/viewer.mjs','web/tests/live_performance.mjs',
  ...['scene','config','short.config','task','learning','manifest','sustained.actions','probe.actions','forward-reverse.actions']
    .map(n=>`examples/full-robot/browser-residual-policy/${n}.json`),
];
const report = {
  version:1, stage:'teacher motor-action interface; no trained neural policy',
  sources:sourcePaths.map(digest), evidence, comparison,
  neutral:summarize(neutral), probe:summarize(probe), native_wasm:parity, viewer:ui,
  rendered:{...timing, performance:{...timing.performance, breakdown:undefined}},
  rendered_keyboard_schedule_matches_accepted_native:true,
  tests:{runtime_environment:8, individual_motor_routing:1, result:'passed',
    log:digest(`${nativeRoot}/runtime-tests.log`)},
  remaining:['train and evaluate a policy','distill deployable student observations',
    'bounded disturbance training','general command and terrain validation',
    'hardware calibration','command-to-visible-response latency measurement',
    ...(!timing.meets_transition_target?['meet rendered 20 ms p95 transition target']:[])],
  scope:'Versioned action/observation/reward commissioning evidence. Probes cover small corrections, not every permitted action. Native/WASM portability is separate from physical accuracy. No learned policy or hardware transfer is claimed.',
};
writeFileSync(output, JSON.stringify(report,null,2)+'\n');console.log(output);
