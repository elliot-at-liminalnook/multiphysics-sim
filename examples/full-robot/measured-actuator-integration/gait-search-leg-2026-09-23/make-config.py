"""Gait search refined by the physical-leg runs of 2026-09-23 and sped up.

From the measured-actuator study (gait-search-measured-2026-09-23), plus:

* Physical-leg tracking screen. On the real leg at full effort and speed
  (measurements/gait-runs/run-1790199348224.json, 3 min of the best gait),
  tracking error ≈ effective delay × commanded (governed) speed. The effective
  delays per joint role come from that run (RMS and peak). Candidates whose
  predicted leg error exceeds 7° RMS or 18° peak (better than the best gait
  measured on the leg, 7.5° / 17.9° at the knee) are rejected before any physics.
* The motors averaged 7–24% effort and never reached the PWM ceiling, so the
  real limit is tracking, not power: slower governors are opened to the
  search (speed floor 0.7 rad/s, acceleration floor 3.5 rad/s²). The baseline
  uses those floors so that it passes the leg screen.
* Speed: 4 optimizer streams in parallel (results per stream unchanged),
  the shared baseline simulated once, trials stopped as soon as a partial
  evaluation shows an irreversible rejection. run.sh tries faster qualified
  physics profiles first.

    python3 make-config.py PROFILE.json QUALIFICATION_DIR   (run from this directory)
"""
import json, math, sys

SRC = '../gait-search-measured-2026-09-23'
c = json.load(open(f'{SRC}/comparison-config.json'))
recipe = c['recipe']
recipe['leg_tracking_screen'] = {
    'delays_s': {
        'Foot servo output': [0.288, 0.300],
        'Worm servo output': [0.113, 0.215],
        'Hip servo output': [0.153, 0.206],
    },
    # At least as trackable as the best gait measured on the leg today
    # (knee 7.5° RMS, 17.9° peak); the simulation's own 5°/15° gates are not
    # yet reachable on the leg's host-controlled path.
    'maximum_rms_rad': math.radians(7.0),
    'maximum_peak_rad': math.radians(18.0),
    'evidence': 'examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/measurements/gait-runs/run-1790199348224.json (knee/worm/hip, effort 100%, speed 100%, 181 s of gait time)',
}
for p in recipe['template']['space']['parameters']:
    if p['name'] == 'governor_speed_rad_s':
        p['bounds'][0] = 0.7
    if p['name'] == 'governor_acceleration_rad_s2':
        p['bounds'][0] = 3.5
c['baseline'] = dict(c['baseline'], cadence_scale=0.75, governor_speed_rad_s=0.7, governor_acceleration_rad_s2=3.5)
c['optimizer_seeds'] = [3501, 3502]
c['attempts_per_algorithm_seed'] = 40
c['settings'] = dict(c['settings'], seed=3501)
c['screening'] = {'parallel': 4, 'max_candidates_per_trial': 48}
c['parallel_streams'] = 4
c['early_rejection_check_s'] = 0.5
if len(sys.argv) == 3:
    c['profile'] = json.load(open(sys.argv[1]))
    c['qualification_directory'] = sys.argv[2]
json.dump(c, open('comparison-config.json', 'w'))
print('wrote comparison-config.json')
