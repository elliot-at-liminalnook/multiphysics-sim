"""Build this study's config from the 2026-09-19 comparison config.

Changes (audit of 2026-09-19, see README): tightened search bounds from that
study's feasible evidence, reference-governor limits as search parameters,
coarse reference-speed pre-screen, feasibility-weighted Bayesian proposals and
screened proposals (candidates prepared in parallel; only physics trials use a
slot). Physics, controller code, gates and the qualified baseline are unchanged.

    python3 make-config.py   (run from this directory)
"""
import json, copy

SRC = '../gait-search-comparison-2026-09-19'
c = json.load(open(f'{SRC}/comparison-config.json'))
recipe = c['recipe']
space = recipe['template']['space']['parameters']
bounds = {
    # Every attempt above +3 mm failed inverse kinematics (33 attempts).
    'body_height_offset_m': [-0.03, 0.005],
    # No attempt above 0.98 completed; the 1.0 edge keeps a margin.
    'cadence_scale': [0.75, 1.0],
    # No attempt above 0.71 completed; 0.75 keeps a margin.
    'stance_fraction': [0.6, 0.75],
}
for p in space:
    if p['name'] in bounds:
        p['bounds'] = bounds[p['name']]
governor = recipe['experiment']['scene']['controller']['parameters']['reference_governor']
space += [
    # From the current 80 deg/s (search floor) to the servo's no-load speed.
    {'name': 'governor_speed_rad_s', 'kind': 'AngularVelocity', 'bounds': [governor['maximum_speed_rad_s'], 5.5], 'integer': False},
    {'name': 'governor_acceleration_rad_s2', 'kind': 'AngularAcceleration', 'bounds': [governor['maximum_acceleration_rad_s2'], 60.0], 'integer': False},
]
recipe['policy_bindings'] = [
    {'parameter': 'governor_speed_rad_s', 'pointer': '/reference_governor/maximum_speed_rad_s'},
    {'parameter': 'governor_acceleration_rad_s2', 'pointer': '/reference_governor/maximum_acceleration_rad_s2'},
]
recipe['coarse_speed_screen'] = {'factor': 8, 'margin': 1.1}
c['baseline'] = dict(c['baseline'], governor_speed_rad_s=governor['maximum_speed_rad_s'],
                     governor_acceleration_rad_s2=governor['maximum_acceleration_rad_s2'])
c['optimizer_seeds'] = [3301]
c['attempts_per_algorithm_seed'] = 33
c['settings'] = dict(c['settings'], seed=3301, initial_design=len(space) + 1, feasibility_candidates=4, feasibility_length_scale=0.15)
c['screening'] = {'parallel': 8, 'max_candidates_per_trial': 48}
c['qualification_directory'] = 'examples/full-robot/measured-actuator-integration/gait-search-screened-2026-09-23/qualification'
json.dump(recipe['template'], open('contact-template.json', 'w'), indent=1)
json.dump(c, open('comparison-config.json', 'w'))
print('wrote comparison-config.json and contact-template.json;', len(space), 'parameters')
