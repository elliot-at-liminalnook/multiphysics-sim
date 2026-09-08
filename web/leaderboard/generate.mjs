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
  const benchmark = simulated >= 60 ? 'flat-forward-stop-60s-v1' : 'flat-forward-stop-24s-v1';
  const numerical = d.numerical ? read(d.numerical) : null;
  const footDifference = numerical?.metrics.foot_marker_position_m.maximum;
  const bodyDifference = numerical?.metrics.body_position_m?.maximum;
  const numericalStatus = !numerical ? 'missing' : footDifference > .001 || bodyDifference > .0005 ? 'fail' :
    Number.isFinite(footDifference) && Number.isFinite(bodyDifference) ? 'pass' : 'missing';
  const gate = (status, detail) => ({status, detail});
  const performance = d.performance?.performance;
  const gates = {
    sustained_walk: gate(simulated < 60 ? 'missing' : a.passed ? 'pass' : 'fail', simulated < 60 ? 'Only a short forward/stop case is associated with this recipe.' : '60-second supported-swing, sampled geometry and endpoint audit.'),
    turn_reverse_stop: gate(d.commands ? 'pass' : d.commandsFailed ? 'fail' : 'missing', d.commands ? 'The associated controller has a passing forward/turn/reverse/stop case.' : d.commandsFailed ? 'Live turning loses planned static support before the reverse command is reached; native inputs reproduce the failure.' : 'Full steering coverage at this speed is not associated with this entry.'),
    disturbances: gate('missing', 'Broader held-out disturbance requirements are not yet established.'),
    terrain: gate('missing', 'This entry is a flat-floor experiment; progressively harder terrain is outstanding.'),
    numerical_accuracy: gate(numericalStatus, numerical ? `Sampled foot difference ${(footDifference * 1000).toFixed(3)} mm; required at most 1 mm, plus a 0.5 mm body screen. ${Number.isFinite(bodyDifference) ? `Body difference ${(bodyDifference * 1000).toFixed(3)} mm.` : 'Body screen not recorded.'}` : 'Solver-option agreement does not establish timestep convergence.'),
    browser_realtime: gate(!d.performance ? 'missing' : performance?.active_motion?.simulation_per_wall_second >= 1 && performance?.active_motion?.transition_p95_s <= .020 ? 'pass' : 'fail', 'Requires active simulation/wall >=1 and active transition p95 <=20 ms on the recorded browser host.'),
    replay_parity: gate(d.parity ? 'pass' : 'missing', d.parity ? 'Associated native/WASM comparison and same-host replay pass.' : d.partialParity ? 'The 24-second teacher case passes native/WASM comparison and exact replay/reset; full-minute host parity is not yet established.' : 'No browser parity result is associated with this exact controller profile.'),
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
    browser_host: d.performance?.host ?? null, gates,
    evidence: [source(d.evidence), source(omittedPath), ...(d.numerical ? [source(d.numerical)] : []), ...(d.extraEvidence ?? []).map(source)],
    capture: source(d.capture), measurement: {...metrics, motors: undefined, phases: undefined, feet: undefined},
    limitations: 'Uncalibrated CAD-derived simulation, ideal observations and a privileged planner. Sampled marker motion and positive shaft work do not certify slip-free contact or hardware energy use.'};
  entries.push(validateEntry(entry));
}
writeFileSync('web/leaderboard/evaluations.json', JSON.stringify({version: 1, entries,
  generator: source('web/leaderboard/generate.mjs'), scope: 'Development evidence. No entry currently satisfies the complete declared gate set; speed rankings remain empty.'}, null, 2) + '\n');
console.log(entries.map(e => ({id: e.id, sustained_speed_mm_s: e.metrics.sustained_speed_m_s == null ? null : e.metrics.sustained_speed_m_s * 1000, task: e.metrics.task_passed})));
