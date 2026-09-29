"""Fast measured-actuator gait search at the bench supply (2026-09-25).

From gait-search-measured-2026-09-23 (accepted actuator registry, 3.6 s
episodes, strict gates), changed to explore many more gaits per hour and to
allow faster gaits:

* Motor supply 12.5 V (was 11.1 V, a nominal 3S battery): the operator's
  bench supply (WANPTEK DPS3010U, CV). Experimental override of every
  simulated motor's supply voltage; CAD and the registry are unchanged.
  Registry-derived speed limits follow the supply automatically.
* Governor speed up to 1.0 x slowest joint (was 0.8).
* Cadence scale 0.4-1.0 (was 0.6-1.0) and stride 0.08-0.26 m (was 0.22):
  3401-Bayesian-019 sat on the cadence floor with a 0.173 m stride.
* Baseline = 3401-Bayesian-019's values.
* Throughput: 8 parallel optimizer streams (4 seeds x Bayesian/CMA-ES) on the
  8-core host; task-level reduced model (quasistatic winding, 32/16/8 x step,
  the fastest that qualifies against the detailed baseline); 1024 reference
  samples (512 failed the interpolation check on this faster baseline); coarse speed screen; early rejection every 0.5 s; minimal
  per-trial artifacts. Gates unchanged (upright 0.9, 5 deg RMS, 15 deg peak).

    python3 make-config.py [PROFILE.json QUALIFICATION_DIR]   (run from this directory)
"""
import json, sys

SRC = '../gait-search-measured-2026-09-23'
SUPPLY_V = 12.5
c = json.load(open(f'{SRC}/comparison-config.json'))
recipe = c['recipe']

def supply(node):
    n = 0
    if isinstance(node, dict):
        for k, v in node.items():
            if k in ('supply_voltage', 'supply_voltage_v') and isinstance(v, (int, float)):
                node[k] = SUPPLY_V; n += 1
            else:
                n += supply(v)
    elif isinstance(node, list):
        for v in node:
            n += supply(v)
    return n

changed = supply(recipe['experiment']) + supply(recipe['planning_scene'])
assert changed > 0, 'no motor supply voltage found'
recipe['actuator_limits'] = dict(recipe['actuator_limits'], governor_speed_fraction=1.0)
for p in recipe['template']['space']['parameters']:
    if p['name'] == 'cadence_scale':
        p['bounds'] = [0.4, 1.0]
    if p['name'] == 'stride_m':
        p['bounds'] = [0.08, 0.26]
best = json.load(open(f'{SRC}/comparison/3401-Bayesian-019/proposal.json'))['values']
c['baseline'] = {k: best[k] for k in c['baseline']}
recipe['compiler']['robot']['uniform_samples'] = 1024
recipe['coarse_speed_screen'] = {'factor': 4, 'margin': 1.1}
c['optimizer_seeds'] = [3701, 3702, 3703, 3704]
c['attempts_per_algorithm_seed'] = 60
c['settings'] = dict(c['settings'], seed=3701)
c['screening'] = {'parallel': 2, 'max_candidates_per_trial': 48}
c['parallel_streams'] = 8
c['early_rejection_check_s'] = 0.5
c['minimal_artifacts'] = True
c['qualification_directory'] = 'examples/full-robot/measured-actuator-integration/gait-search-fast-12v5-2026-09-25/qualification'
if len(sys.argv) == 3:
    c['profile'] = json.load(open(sys.argv[1]))
    c['qualification_directory'] = sys.argv[2]
json.dump(c, open('comparison-config.json', 'w'))
print('wrote comparison-config.json;', changed, 'supply voltages set to', SUPPLY_V, 'V')
