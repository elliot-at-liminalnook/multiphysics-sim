import {mkdtempSync, writeFileSync, readFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import test from 'node:test';

test('rolling material contact is stationary despite COM motion; added slip dissipates shear work', () => {
  const dir = mkdtempSync(join(tmpdir(), 'contact-motion-'));
  try {
    const measure = slip => {
      const r = {completed: true, error: null,
        recording: {scene: {robot: {world: {terrain: null}, links: [{name: 'rolling-foot'}]}},
          config: {policy: {point_feedback: {markers: [{id: 'foot', link: 'rolling-foot'}]}}}},
        frames: [0, .02].map(time_s => {
          const x = (.01 + slip) * time_s;
          return {time_s, poses: [{name: 'rolling-foot', position_m: [x, 0, .01],
            velocity_m_s: [.01 + slip, 0, 0], angular_velocity_rad_s: [0, 1, 0]}],
            contacts: [{link: 0, other: null, point_m: [x, 0, 0], force_n: [-2, 0, 10]},
              // Internal forces must not be counted as floor traction.
              {link: 0, other: 1, point_m: [x, 0, 1], force_n: [1000, 0, 1000]}]};
        })};
      const input = join(dir, 'capture.json'), output = join(dir, 'report.json');
      writeFileSync(input, JSON.stringify(r));
      execFileSync(process.execPath, ['examples/interactive/analyze_floor_contact_motion.mjs', input, output], {stdio: 'pipe'});
      return JSON.parse(readFileSync(output)).feet[0];
    };
    const rolling = measure(0);
    assert.equal(rolling.integrated_load_weighted_tangential_speed_m, 0);
    assert.equal(rolling.sampled_translational_shear_dissipation_j, 0);
    const sliding = measure(.002);
    assert(Math.abs(sliding.integrated_load_weighted_tangential_speed_m - .00004) < 1e-15);
    assert(Math.abs(sliding.sampled_translational_shear_dissipation_j - .00008) < 1e-15);
    assert.equal(sliding.sampled_translational_shear_supplied_work_j, 0);
  } finally { rmSync(dir, {recursive: true, force: true}); }
});
