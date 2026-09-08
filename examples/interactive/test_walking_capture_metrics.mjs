import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, writeFileSync, readFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';

test('measures net travel separately from oscillation, sampled loading and mechanical work', () => {
  const directory = mkdtempSync(join(tmpdir(), 'walking-metrics-'));
  try {
    const rotation = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
    const frames = Array.from({length: 601}, (_, i) => {
      const time = i * .02, body = [.002 * time, .001 * Math.sin(2 * Math.PI * time), 0];
      return {time_s: time, poses: [{name: 'body', position_m: body, rotation},
        {name: 'foot', position_m: [.001 * time, 0, 0], rotation}],
        policy: i === 0 ? null : {step_reference: {reference: {phase: 'return', body_world_m: body, waiting: false}}},
        policy_inputs: [.002, 0, 0],
        contacts: time < 6 ? [{link: 1, other: null, force_n: [0, 0, 2]}] : [],
        servo_targets_rad: [.01], reference_targets_rad: [0], joint_positions: [0, .005],
        motor_readings: [{gear_speed_rad_s: 1, shaft_torque_nm: 2}]};
    });
    const capture = {completed: true, error: null, wall_s: 1, transition_wall_s: [.01, .02], frames,
      metadata: {coordinate_names: ['joint'], joint_indices: [1]},
      recording: {scene: {robot: {links: [{name: 'body'}, {name: 'foot'}]},
        controller: {inputs: [{name: 'forward'}, {name: 'lateral'}, {name: 'yaw'}]}},
      config: {policy: {body_feedback: {reference_link: 'body'},
        point_feedback: {markers: [{id: 'foot-marker', link: 'foot', local_point_m: [0, 0, 0]}]},
        step_reference: {command_channels: ['forward', 'lateral', 'yaw']}}}}};
    const input = join(directory, 'capture.json'), output = join(directory, 'report.json');
    writeFileSync(input, JSON.stringify(capture));
    execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs', input, output]);
    const report = JSON.parse(readFileSync(output));
    assert(Math.abs(report.sustained_windows[0].measured_sustained_speed_m_s - .002) < 1e-14);
    assert(Math.abs(report.net_horizontal_displacement_m - .024) < 1e-14);
    assert(report.horizontal_body_path_m > 2 * report.net_horizontal_displacement_m);
    assert(Math.abs(report.positive_mechanical_work_j - 24) < 1e-10);
    assert(Math.abs(report.feet[0].sampled_loaded_tangential_path_m - .00598) < 1e-12);
    assert.equal(report.motors[0].maximum_tracking_error_rad, .005);
    assert.equal(report.phases.initial.duration_s, .02);
    assert.equal(report.native_compute.transition_p95_s, .02);
  } finally { rmSync(directory, {recursive: true, force: true}); }
});
