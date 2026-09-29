"""Fast, distance-focused gait search for the physical leg (2026-09-23).

From gait-search-leg-2026-09-23 (physical-leg tracking screen, measured
motor families, 4 parallel optimizer streams, shared baseline, early
rejection), relaxed for speed as agreed:

* Objective unchanged in form: eligible forward speed = distance along the
  diagonal / episode time, but over 8 s episodes (was 3.6 s), so gaits must
  keep going.
* Tilt no longer matters: the body-orientation gate only rejects a body
  turned past horizontal (minimum up-z 0.0, was 0.9). Falls and body-floor
  contact still reject.
* Simulated tracking gates relaxed to 10° RMS / 30° peak (were 5° / 15°);
  the physical-leg tracking screen (7° / 18° predicted on the real leg)
  still applies before physics.
* Lower fidelity for speed: 512 reference samples (was 2048; preparation
  52 s -> 29 s), task-level qualification (the fast model must reach the
  same outcome and a walking score within 25% of the detailed model's, not
  every trajectory and current), and run.sh tries fast profiles first.
* Minimal per-trial artifacts (spec hashes; captures only for passing trials).

    python3 make-config.py [PROFILE.json QUALIFICATION_DIR]   (run from this directory)
"""
import json, sys

SRC = '../gait-search-leg-2026-09-23'
c = json.load(open(f'{SRC}/comparison-config.json'))
recipe = c['recipe']
EPISODE_S = 8.0
exp = recipe['experiment']
period = exp['scene']['period_s']
rows = int(round(EPISODE_S / period))
names = [i['name'] for i in exp['scene']['controller']['inputs']]
seq = names.index('command.packet_sequence')
last = exp['source_actions'][-1]
while len(exp['source_actions']) < rows:
    row = list(last)
    row[seq] = len(exp['source_actions']) + 1
    exp['source_actions'].append(row)
exp['source_actions'] = exp['source_actions'][:rows]
exp['scene']['duration_s'] = EPISODE_S
exp['config']['steps'] = int(round(EPISODE_S / exp['config']['step_s']))
recipe['compiler']['robot']['uniform_samples'] = 512
recipe['coarse_speed_screen'] = {'factor': 4, 'margin': 1.1}
c['gates'] = dict(c['gates'], minimum_body_up_z=0.0,
                  maximum_tracking_rms_rad=0.17453292519943295,
                  maximum_tracking_peak_rad=0.5235987755982988)
c['optimizer_seeds'] = [3601, 3602]
c['attempts_per_algorithm_seed'] = 60
c['settings'] = dict(c['settings'], seed=3601)
c['screening'] = {'parallel': 4, 'max_candidates_per_trial': 48}
c['parallel_streams'] = 4
c['early_rejection_check_s'] = 0.5
c['minimal_artifacts'] = True
if len(sys.argv) == 3:
    c['profile'] = json.load(open(sys.argv[1]))
    c['qualification_directory'] = sys.argv[2]
json.dump(c, open('comparison-config.json', 'w'))
print('wrote comparison-config.json;', rows, 'action rows,', exp['config']['steps'], 'steps')
