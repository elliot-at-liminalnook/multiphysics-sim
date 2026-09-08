// Materialize compact benchmark evidence from actual native captures. Build
// packaging needs only these versioned results and their pinned recipe inputs.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {validateEntry} from '../viewer/leaderboard-model.mjs';
const read = p => JSON.parse(readFileSync(p));
const hash = v => createHash('sha256').update(v).digest('hex');
const source = path => ({path, sha256: hash(readFileSync(path))});
const canonical = v => JSON.stringify(v, (_, x) => x && !Array.isArray(x) && typeof x === 'object' ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => a.localeCompare(b))) : x);
const browserPath = 'examples/full-robot/browser-precision/browser-status.json', browser = read(browserPath);
const fastPath = 'examples/full-robot/student-speed/validation-status.json', fast = read(fastPath).cases.find(c => c.name === '2x-support-minute');
const halfPath = 'examples/full-robot/swing-advance/browser-status.json', half = read(halfPath);
const wholePath = 'examples/full-robot/whole-swing/finish-status.json', whole = read(wholePath).cases.find(c => c.name === 'finish-0.85-20ms');
const wholeParityPath = 'examples/full-robot/whole-swing/browser-parity.json', wholeParity = read(wholeParityPath);
const wholeBrowserPath = 'examples/full-robot/whole-swing/browser-status.json', wholeBrowser = read(wholeBrowserPath);
const teacherPath = 'examples/full-robot/whole-swing/sustained-status.json', teacher = read(teacherPath).cases.find(c => c.name === 'teacher-minute-5ms');
const teacherParityPath = 'examples/full-robot/whole-swing/teacher-browser-parity.json';
const turnPath = 'examples/full-robot/whole-swing/turn-support-status.json', turn = read(turnPath).cases.find(c => c.name === 'student-turn-20ms-support-24mm');
const combinedPath = 'examples/full-robot/whole-swing/combined-status.json', combined = read(combinedPath).cases.find(c => c.name === 'combined-minute-2.5ms');
const broydenPath = 'examples/full-robot/whole-swing/broyden-status.json', broyden = read(broydenPath).cases.find(c => c.name === 'student-turn-20ms-broyden');
const broydenBrowserPath = 'examples/full-robot/whole-swing/broyden-browser-status.json', broydenBrowser = read(broydenBrowserPath);
const refinedTeacherPath = 'examples/full-robot/whole-swing/minute-refinement-status.json', refinedTeacher = read(refinedTeacherPath).cases.find(c => c.name === 'combined-minute-1.25ms');
const referenceBrowserPath = 'examples/full-robot/whole-swing/reference-browser-status.json', referenceBrowser = read(referenceBrowserPath);
const tangentPath = 'examples/full-robot/whole-swing/tangent-probes-status.json', tangent = read(tangentPath).cases.find(c => c.name === 'tangent-probes-enabled');
const tangentInitialBrowserPath = 'examples/full-robot/whole-swing/tangent-probes-initial-browser.json', tangentInitialBrowser = read(tangentInitialBrowserPath);
const radiusPath = 'examples/full-robot/whole-swing/tangent-radius-status.json';
const radiusSelectionPath = 'examples/full-robot/whole-swing/tangent-radius-selection.json', radiusSelection = read(radiusSelectionPath);
const radius = read(radiusPath).cases.find(c => c.name === radiusSelection.selected.name);
const radiusParityPath = 'examples/full-robot/whole-swing/tangent-radius-browser-parity.json';
const radiusParity = read(radiusParityPath).cases.find(c => c.name === radius.name).measurement;
const tangentBrowserPath = 'examples/full-robot/whole-swing/tangent-browser-status.json', tangentBrowser = read(tangentBrowserPath);
const transportPath = 'examples/full-robot/whole-swing/frame-transport-status.json', transport = read(transportPath);
const transportIntegrityPath = 'examples/full-robot/whole-swing/frame-transport-integrity.json';
assert(transport.complete && read(transportIntegrityPath).passed);
const exactBasePath = 'examples/full-robot/whole-swing/exact-probe-base-status.json';
const exactBase = read(exactBasePath).cases.find(c => c.name === 'exact-probe-base-enabled');
const exactBaseParityPath = 'examples/full-robot/whole-swing/exact-probe-base-browser-parity.json', exactBaseParity = read(exactBaseParityPath);
assert(read('examples/full-robot/whole-swing/exact-probe-base-identity.json').passed);
const exactBaseBrowserPath = 'examples/full-robot/whole-swing/exact-probe-base-browser-status.json', exactBaseBrowser = read(exactBaseBrowserPath);
const exactBaseBrowserIntegrityPath = 'examples/full-robot/whole-swing/exact-probe-base-browser-integrity.json';
assert(exactBaseBrowser.complete && read(exactBaseBrowserIntegrityPath).passed);
const distilledPath = 'examples/full-robot/whole-swing/fast-distillation-evaluation-status.json', distilled = read(distilledPath);
const distilledMinute = distilled.cases.find(c => c.name === 'fitted-minute');
const distilledSummaryPath = 'examples/full-robot/whole-swing/fast-distillation-summary.json';
const distilledFidelityPath = 'examples/full-robot/whole-swing/fast-student-fidelity-status.json', distilledFidelity = read(distilledFidelityPath);
const distilledTurn = distilledFidelity.cases.find(c => c.name === 'turn-20ms');
assert(distilled.complete && distilledFidelity.complete);
const distilledBrowserPath = 'examples/full-robot/whole-swing/fast-student-browser-status.json', distilledBrowser = read(distilledBrowserPath);
const distilledBrowserIntegrityPath = 'examples/full-robot/whole-swing/fast-student-browser-integrity.json';
assert(distilledBrowser.complete && read(distilledBrowserIntegrityPath).passed);
const definitions = [
  {id: 'browser-crawl-minute', name: 'Browser crawl', description: 'The slower heading student. Its native minute passes walking and stopping; rendered minute processing still misses 20 ms.',
    scene: 'examples/full-robot/student-distillation/scene.json', config: 'examples/full-robot/browser-precision/guarded.config.json',
    capture: browser.minute_walking.capture.path, acceptance: browser.minute_walking, evidence: browserPath,
    performance: browser.episodes.find(e => e.name === 'sustained').report, parity: browser.parity.passed,
    commands: browser.validation.cases?.some(c => c.name === 'turn-reverse' && c.passed) ?? false},
  {id: 'faster-student-minute', name: 'Faster student', description: 'About twice the crawl speed, with the same CAD motor limits. All minute-long swings qualify, but heading and stopping miss their gates.',
    scene: 'examples/full-robot/student-speed/scene.json', config: 'examples/full-robot/student-speed/2x-support-minute.config.json',
    capture: fast.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: fast.acceptance, evidence: fastPath,
    numerical: 'examples/full-robot/student-speed/selected-minute-refinement.json'},
  {id: 'overlapping-forward-stop', name: 'Move during swing', description: 'Advances half the body motion during swing. The short forward/stop test passes, but timestep agreement and active browser timing fail.',
    scene: 'examples/full-robot/student-speed/scene.json', config: 'examples/full-robot/swing-advance/half.config.json',
    capture: 'runs/interactive/swing-advance/live-forward.native.json', acceptance: read('runs/interactive/swing-advance/live-forward-acceptance/summary.json'),
    evidence: halfPath, performance: half.live, parity: half.parity.passed,
    numerical: 'examples/full-robot/swing-advance/half-refinement.json'},
  {id: 'whole-swing-short', name: 'Earlier horizontal finish', description: 'Faster asymmetric stance and overlapping motion. The 3.75 mm/s command passes short forward tasks; turning, refined stopping, trajectory accuracy and browser timing fail.',
    scene: 'examples/full-robot/whole-swing/browser.scene.json', config: 'examples/full-robot/whole-swing/browser.config.json',
    capture: whole.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(whole.acceptance.source.path), evidence: wholePath,
    parity: wholeParity.passed && wholeParity.replay_exact && wholeParity.reset_exact, extraEvidence: [wholeParityPath, wholeBrowserPath],
    performance: wholeBrowser.forward, commandsFailed: !wholeBrowser.turn.completed,
    numerical: 'examples/full-robot/whole-swing/finish-0.85-refinement.json'},
  {id: 'faster-teacher-minute', name: 'Faster feedback teacher', description: 'Three times the baseline travel speed over a minute, with 41 qualified swings. Privileged body/foot feedback and finer physics; stopping and body trajectory agreement still fail.',
    scene: 'examples/full-robot/whole-swing/teacher-minute.scene.json', config: 'examples/full-robot/whole-swing/teacher-minute.config.json',
    capture: teacher.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(teacher.acceptance.source.path), evidence: teacherPath,
    partialParity: true, extraEvidence: [teacherParityPath],
    numerical: 'examples/full-robot/whole-swing/sustained-refinement.json'},
  {id: 'faster-steering-short', name: 'Faster steering student', description: 'Separate forward and turning postures. Native forward/turn/reverse/stop passes at +3.75/-1.25 mm/s command limits; refined stopping and numerical accuracy still fail.',
    scene: 'examples/full-robot/whole-swing/turn-browser.scene.json', config: 'examples/full-robot/whole-swing/turn-browser.config.json',
    capture: turn.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(turn.acceptance.source.path), evidence: turnPath,
    numerical: 'examples/full-robot/whole-swing/turn-student-refinement.json', commands: true, benchmark: 'flat-steering-24s-v1'},
  {id: 'settled-teacher-minute', name: 'Settled feedback teacher', description: 'Minute-long walking and mixed steering pass at 2.5 and 1.25 ms. Added standing feedback waits until the reference settles. Minute-long timestep agreement still misses the body limit; browser speed and robustness remain open.',
    scene: 'examples/full-robot/whole-swing/combined-minute.scene.json', config: 'examples/full-robot/whole-swing/combined-minute.config.json',
    capture: combined.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(combined.acceptance.source.path), evidence: combinedPath,
    commands: true, extraEvidence: ['examples/full-robot/whole-swing/combined-feedback-check.json', 'examples/full-robot/whole-swing/combined-turn-refinement.json'],
    numerical: 'examples/full-robot/whole-swing/combined-minute-refinement.json'},
  {id: 'secant-steering-short', name: 'Steering with secant solver', description: 'Passing 20 ms steering with bounded shared-solver secants. Browser p95 improves to 26.1 ms but still misses 20 ms. Native trajectories match within nanometres; timestep accuracy, sustained walking and robustness remain open.',
    scene: 'examples/full-robot/whole-swing/broyden-turn.scene.json', config: 'examples/full-robot/whole-swing/broyden-turn.config.json',
    capture: broyden.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(broyden.acceptance.source.path), evidence: broydenPath,
    commands: true, benchmark: 'flat-steering-24s-v1', extraEvidence: ['examples/full-robot/whole-swing/broyden-difference.json', broydenBrowserPath],
    performance: broydenBrowser.episodes['secant-turn'], parity: broydenBrowser.parity.passed},
  {id: 'refined-teacher-minute', name: 'Refined walking teacher', description: 'Faster minute-long walking, stopping and mixed steering pass at 1.25 ms. The 0.625 ms comparison meets the declared trajectory screen. This is a fine native reference; realtime browser performance and held-out robustness remain unverified.',
    scene: 'examples/full-robot/whole-swing/reference-minute.scene.json', config: 'examples/full-robot/whole-swing/reference-minute.config.json',
    capture: refinedTeacher.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(refinedTeacher.acceptance.source.path), evidence: refinedTeacherPath,
    numerical: 'examples/full-robot/whole-swing/minute-fine-difference.json', commands: true, parity: referenceBrowser.parity.passed,
    extraEvidence: ['examples/full-robot/whole-swing/combined-status.json', 'examples/full-robot/whole-swing/combined-turn-refinement.json', referenceBrowserPath]},
  {id: 'tangent-steering-short', name: 'Steering with tangent probes', description: 'Uses approximate closure tangents only in derivative probes, with exact accepted physics and exact-derivative fallback. Native steering passes with nanometre solver differences; coarse timestep accuracy still fails.',
    scene: 'examples/full-robot/whole-swing/tangent-turn.scene.json', config: 'examples/full-robot/whole-swing/tangent-turn.config.json',
    capture: tangent.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(tangent.acceptance.source.path), evidence: tangentPath,
    commands: true, benchmark: 'flat-steering-24s-v1', numerical: 'examples/full-robot/whole-swing/tangent-probes-refinement.json',
    parityFailed: !tangentInitialBrowser.parity.passed,
    extraEvidence: ['examples/full-robot/whole-swing/tangent-probes-difference.json', 'examples/full-robot/whole-swing/tangent-probes-profile.json', tangentInitialBrowserPath]},
  {id: 'portable-tangent-steering', name: 'Steering with portable tangent probes', description: 'The larger derivative probe passes native/WASM agreement and exact replay while retaining exact accepted physics. All 15 steering swings pass; coarse timestep accuracy still fails.',
    scene: 'examples/full-robot/whole-swing/portable-tangent-turn.scene.json', config: 'examples/full-robot/whole-swing/portable-tangent-turn.config.json',
    capture: radius.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(radius.acceptance.source.path), evidence: radiusPath,
    commands: true, benchmark: 'flat-steering-24s-v1', numerical: 'examples/full-robot/whole-swing/tangent-radius-refinement.json',
    parity: radiusParity.passed && radiusParity.replay_exact && radiusParity.reset_exact,
    performance: transport.cases.find(c => c.name === 'json-turn').measurement,
    extraEvidence: [radiusSelectionPath, radiusParityPath, radiusSelection.selected.comparison.path, tangentBrowserPath, transportPath, transportIntegrityPath]},
  {id: 'exact-base-steering', name: 'Steering with exact base reuse', description: 'Reuses an identical closure calculation before derivative probes. All 15 steering swings and native/WASM replay checks pass; native physical states are unchanged. Coarse timestep accuracy still fails.',
    scene: 'examples/full-robot/whole-swing/exact-base-turn.scene.json', config: 'examples/full-robot/whole-swing/exact-base-turn.config.json',
    capture: exactBase.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(exactBase.acceptance.source.path), evidence: exactBasePath,
    commands: true, benchmark: 'flat-steering-24s-v1', numerical: 'examples/full-robot/whole-swing/exact-probe-base-refinement.json',
    parity: exactBaseParity.passed && exactBaseParity.replay_exact && exactBaseParity.reset_exact,
    performance: exactBaseBrowser.cases.find(c => c.name === 'enabled-turn').measurement,
    extraEvidence: [exactBaseParityPath, exactBaseBrowserPath, exactBaseBrowserIntegrityPath, 'examples/full-robot/whole-swing/exact-probe-base-identity.json', 'examples/full-robot/whole-swing/exact-probe-base-profile.json', 'examples/full-robot/whole-swing/exact-probe-base-integrity.json']},
  {id: 'fast-distilled-minute', name: 'Faster learned motor feedback', description: 'Distilled from the faster fine-step teacher. Measured minute-long travel is 3.75 mm/s with 41 qualified swings. Fine development steering passes; the reserved mirrored transition fails planned support. The planner still uses ideal state.',
    scene: 'examples/full-robot/whole-swing/fast-distilled-minute.scene.json', config: 'examples/full-robot/whole-swing/fast-distilled-minute.config.json',
    capture: distilledMinute.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(distilledMinute.acceptance.source.path), evidence: distilledPath,
    commands: true, heldoutCommandsFailed: true, numerical: 'examples/full-robot/whole-swing/fast-student-fine-refinement.json',
    extraEvidence: [distilledSummaryPath, 'examples/full-robot/whole-swing/fast-distillation/observation-boundary.json']},
  {id: 'fast-distilled-steering', name: 'Faster learned browser candidate', description: 'The new learned motor feedback passes short 20 ms steering. Minute-long stopping and trajectory refinement fail, so this remains experimental. Encoder/IMU signals and the upstream planner are still ideal simulation.',
    scene: 'examples/full-robot/whole-swing/fast-distilled-turn.scene.json', config: 'examples/full-robot/whole-swing/fast-distilled-turn.config.json',
    capture: distilledTurn.sources.find(s => s.path.endsWith('.native.json')).path, acceptance: read(distilledTurn.acceptance.source.path), evidence: distilledFidelityPath,
    commands: true, benchmark: 'flat-steering-24s-v1', numerical: 'examples/full-robot/whole-swing/fast-student-coarse-refinement.json',
    performance: distilledBrowser.cases.find(c => c.name === 'student-turn').measurement,
    parity: distilledBrowser.parity.passed && distilledBrowser.parity.replay_exact && distilledBrowser.parity.reset_exact,
    extraEvidence: [distilledSummaryPath, distilledBrowserPath, distilledBrowserIntegrityPath, 'examples/full-robot/whole-swing/fast-student-fidelity-summary.json', 'examples/full-robot/whole-swing/fast-student-unused-feedback.json']},
];
const taskPath = 'examples/full-robot/heading-task/task.json';
mkdirSync('runs/leaderboard', {recursive: true});
function authored(a, b, path = 'recipe') {
  if (a && typeof a === 'object') { assert(b && typeof b === 'object', path); if (Array.isArray(a)) assert.equal(a.length, b.length, path);
    for (const k of Object.keys(a)) authored(a[k], b[k], `${path}.${k}`);
  } else assert(a === b, path);
}
const entries = [];
const omittedPath = 'web/leaderboard/unretained-scene-fields.json';
const omittedFields = new Set(read(omittedPath).fields);
function authoredScene(a, b, path = 'scene') {
  if (a && typeof a === 'object') {
    assert(b && typeof b === 'object', path); if (Array.isArray(a)) assert.equal(a.length, b.length, path);
    for (const k of Object.keys(a)) {
      const field = `${path}.${k}`;
      if (!Object.hasOwn(b, k)) assert(omittedFields.has(field.replace(/\.\d+/g, '.*')), `Unrecognized omitted scene field: ${field}`);
      else authoredScene(a[k], b[k], field);
    }
  } else assert(a === b, `Tested physical scene mismatch: ${path}`);
}
for (const d of definitions) {
  const capture = read(d.capture), scene = read(d.scene), config = read(d.config), task = read(taskPath), a = d.acceptance;
  assert(capture.completed && !capture.error); assert.equal(source(d.capture).sha256, a.capture.sha256);
  authored(config, capture.recording.config); authoredScene(scene, capture.recording.scene); assert.deepEqual(task, capture.task);
  assert.equal(scene.robot.source.cad_sha256, capture.recording.scene.robot.source.cad_sha256);
  const metricPath = `runs/leaderboard/${d.id}.metrics.json`;
  execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs', d.capture, metricPath]);
  const metrics = read(metricPath), window = metrics.sustained_windows[0], simulated = metrics.simulated_s;
  const physics = structuredClone(config); for (const k of ['policy', 'steps', 'report_every', 'profile_solver', 'world_loads']) delete physics[k];
  const modelHash = hash(canonical(capture.recording.scene.robot));
  const environmentHash = hash(canonical({world: scene.robot.world, loads: config.world_loads ?? null}));
  const fidelityHash = hash(canonical({options: scene.options, physics}));
  const benchmark = d.benchmark ?? (simulated >= 60 ? 'flat-forward-stop-60s-v1' : 'flat-forward-stop-24s-v1');
  const numerical = d.numerical ? read(d.numerical) : null;
  const footDifference = numerical?.metrics.foot_marker_position_m.maximum;
  const bodyDifference = numerical?.metrics.body_position_m?.maximum;
  const numericalStatus = !numerical ? 'missing' : footDifference > .001 || bodyDifference > .0005 ? 'fail' :
    Number.isFinite(footDifference) && Number.isFinite(bodyDifference) ? 'pass' : 'missing';
  const gate = (status, detail) => ({status, detail});
  const performance = d.performance?.performance;
  const gates = {
    sustained_walk: gate(simulated < 60 ? 'missing' : a.passed ? 'pass' : 'fail', simulated < 60 ? 'Only a short forward/stop case is associated with this recipe.' : '60-second supported-swing, sampled geometry and endpoint audit.'),
    turn_reverse_stop: gate(d.heldoutCommandsFailed ? 'fail' : d.commands ? 'pass' : d.commandsFailed ? 'fail' : 'missing', d.heldoutCommandsFailed ? 'Development steering passes, but the predeclared mirrored-turn-to-forward sequence fails planned static support at 14.84 s.' : d.commands ? 'The associated controller has a passing forward/turn/reverse/stop case.' : d.commandsFailed ? 'Live turning loses planned static support before the reverse command is reached; native inputs reproduce the failure.' : 'Full steering coverage at this speed is not associated with this entry.'),
    disturbances: gate('missing', 'Broader held-out disturbance requirements are not yet established.'),
    terrain: gate('missing', 'This entry is a flat-floor experiment; progressively harder terrain is outstanding.'),
    numerical_accuracy: gate(numericalStatus, numerical ? `Sampled foot difference ${(footDifference * 1000).toFixed(3)} mm; required at most 1 mm, plus a 0.5 mm body screen. ${Number.isFinite(bodyDifference) ? `Body difference ${(bodyDifference * 1000).toFixed(3)} mm.` : 'Body screen not recorded.'}` : 'Solver-option agreement does not establish timestep convergence.'),
    browser_realtime: gate(!d.performance ? 'missing' : performance?.active_motion?.simulation_per_wall_second >= 1 && performance?.active_motion?.transition_p95_s <= .020 ? 'pass' : 'fail', 'Requires active simulation/wall >=1 and active transition p95 <=20 ms on the recorded browser host.'),
    replay_parity: gate(d.parity ? 'pass' : d.parityFailed ? 'fail' : 'missing', d.parity ? 'Associated native/WASM comparison and same-host replay pass.' : d.parityFailed ? 'The associated native/WASM comparison exceeds the declared portability tolerance; see the recorded differences. Exact same-host replay alone does not pass this gate.' : d.partialParity ? 'The 24-second teacher case passes native/WASM comparison and exact replay/reset; full-minute host parity is not yet established.' : 'No browser parity result is associated with this exact controller profile.'),
  };
  const data = {scene, config, task, seed: capture.recording.seed};
  const entry = {id: d.id, name: d.name, description: d.description, benchmark_version: benchmark,
    controller_sha256: hash(canonical({controller: scene.controller, policy: config.policy})), model_sha256: modelHash,
    cad_sha256: scene.robot.source.cad_sha256, environment_sha256: environmentHash, fidelity_sha256: fidelityHash,
    fidelity_label: `Effective servo · dynamic floor · ${config.step_s * 1000} ms`,
    environment_label: config.world_loads?.pulses?.length ? 'Flat floor · declared push' : 'Flat floor · unforced',
    comparison_group: hash(canonical({modelHash, environmentHash, fidelityHash, task, benchmark})),
    load: {scene: source(d.scene), config: source(d.config), task: source(taskPath), seed: data.seed, asset_sha256: hash(JSON.stringify(data))},
    replay: {completed_steps: capture.recording.completed_steps, input_events: capture.recording.input_events ?? []},
    metrics: {simulated_s: simulated, sustained_speed_m_s: simulated >= 60 && window ? window.measured_sustained_speed_m_s : null,
      speed_window_s: window ? window.end_s - window.start_s : 0, short_speed_m_s: window?.measured_sustained_speed_m_s ?? null,
      task_passed: a.passed, qualified_swings: a.lifts.filter(l => l.passed).length, swings: a.lifts.length,
      final_heading_error_rad: a.final_yaw_error_rad, final_position_error_m: a.final_body_error_m,
      positive_mechanical_work_j: metrics.positive_mechanical_work_j,
      native_throughput: metrics.native_compute.simulation_per_wall, native_hardware: 'Not recorded with this capture; not a reference benchmark.',
      browser_active_throughput: performance?.active_motion?.simulation_per_wall_second ?? null,
      browser_active_p95_s: performance?.active_motion?.transition_p95_s ?? null},
    browser_host: d.performance?.host ?? null, browser_runtime: d.performance?.runtime_build ?? null, gates,
    evidence: [source(d.evidence), source(omittedPath), ...(d.numerical ? [source(d.numerical)] : []), ...(d.extraEvidence ?? []).map(source)],
    capture: source(d.capture), measurement: {...metrics, motors: undefined, phases: undefined, feet: undefined},
    limitations: 'Uncalibrated CAD-derived simulation, ideal observations and a privileged planner. Sampled marker motion and positive shaft work do not certify slip-free contact or hardware energy use.'};
  entries.push(validateEntry(entry));
}
writeFileSync('web/leaderboard/evaluations.json', JSON.stringify({version: 1, entries,
  generator: source('web/leaderboard/generate.mjs'), scope: 'Development evidence. No entry currently satisfies the complete declared gate set; speed rankings remain empty.'}, null, 2) + '\n');
console.log(entries.map(e => ({id: e.id, sustained_speed_mm_s: e.metrics.sustained_speed_m_s == null ? null : e.metrics.sustained_speed_m_s * 1000, task: e.metrics.task_passed})));
