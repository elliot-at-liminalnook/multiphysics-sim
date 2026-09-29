"""Build this study's config: the screened study (2026-09-23) with every
motor-dependent value derived from the accepted actuator registry.

The recipe carries `actuator_limits`; every host (compare_gait_search,
prepare_gait_candidate, audit_gait_screens, prepare_contact_motion) calls
`Recipe::sync_actuators` at load, which binds each motor to its role's
accepted family (knee and hip measured 2026-09-23, worm provisional) and
derives the reference-speed screen, planner actuator model and governor
search bounds from the resolved profiles. The copies in this file are
overwritten at load; the study records actuator-provenance.json.

    python3 make-config.py   (run from this directory)
"""
import json

SRC = '../gait-search-screened-2026-09-23'
c = json.load(open(f'{SRC}/comparison-config.json'))
c['recipe']['actuator_limits'] = {
    'registry': 'examples/actuators/hx30hm/accepted/registry.json',
    # Full-drive speed after friction at the simulated supply; the coarse
    # screen keeps its 1.1 margin.
    'screen_speed_fraction': 1.0,
    # Governor speed ≤ 0.8 × the slowest joint (headroom for tracking).
    'governor_speed_fraction': 0.8,
    # Governor acceleration ≤ 0.5 × the lowest measured acceleration envelope.
    'governor_acceleration_fraction': 0.5,
}
# The earlier qualified baseline (stride 0.15 m, cadence 0.8) asks the knee
# for 5.08 rad/s of reference speed; the measured knee delivers 4.40 rad/s at
# the simulated 11.1 V supply, so it is infeasible for the real leg. The new
# baseline shortens the stride; slower cadences are opened to the search.
c['baseline'] = dict(c['baseline'], stride_m=0.12)
for p in c['recipe']['template']['space']['parameters']:
    if p['name'] == 'cadence_scale':
        p['bounds'] = [0.6, 1.0]
c['optimizer_seeds'] = [3401, 3402]
c['attempts_per_algorithm_seed'] = 40
c['settings'] = dict(c['settings'], seed=3401)
c['qualification_directory'] = 'examples/full-robot/measured-actuator-integration/gait-search-measured-2026-09-23/qualification'
json.dump(c, open('comparison-config.json', 'w'))
print('wrote comparison-config.json')
