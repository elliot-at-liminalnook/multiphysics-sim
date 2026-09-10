"""Bounded CAD-side derivation of local collision detail; source CAD is read-only."""
import argparse
import hashlib
import json
import time
import zipfile
from pathlib import Path

import numpy as np
from robocad.kernel import default_kernel
from robocad.physical import _weld_np, signed_distance_grid, solid_collision_meshes

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output-dir',type=Path,default=Path('examples/full-robot/fast-wasd'))
parser.add_argument('--source',type=Path,default=Path('examples/full-robot/whole-swing/settled-integral-candidate.scene.json'))
parser.add_argument('--probes',type=Path,default=Path('examples/full-robot/gait-exploration/cad-point-check.json'))
parser.add_argument('--link',default='-X | Hip output shaft and pulley')
args = parser.parse_args()
out,source = args.output_dir,args.source
out.mkdir(parents=True,exist_ok=True)
cad = Path('examples/full-robot/baseline/robot.rcad')
scene = json.loads(source.read_text())
assert hashlib.sha256(cad.read_bytes()).hexdigest() == scene['robot']['source']['cad_sha256']
probes = json.loads(args.probes.read_text())
assert probes['cad_sha256'] == scene['robot']['source']['cad_sha256']
name = args.link
link = next(l for l in scene['robot']['links'] if l['name'] == name)
points = np.array([r['cad_point_world_mm'] for r in probes['rows'] if r['target_link'] == name]) / 1000 - link['com']
assert points.size and all(not r['inside_any_solid'] and not r['on_any_solid'] for r in probes['rows'] if r['target_link'] == name)
center = points.mean(axis=0)
half = np.array([.006, .004, .006])
lo, hi = center - half, center + half
parent = link['collision']['sdf']
assert np.all(lo >= parent['origin'])
assert np.all(hi <= np.array(parent['origin']) + parent['cell'] * (np.array(parent['dims']) - 1))
kernel = default_kernel()
started = time.monotonic()
meshes, solids = [], []
with zipfile.ZipFile(cad) as archive:
    nodes = {n['id']: n for n in json.loads(archive.read('manifest.json'))['nodes']}
    for member in link['members']:
        body = kernel.deserialize(archive.read(f'brep/{member}.brep'), nodes[member].get('body_kind', 'solid'))
        v, t = _weld_np(kernel.tessellate(body, .15))
        meshes.append((v * .001 - link['com'], t))
        solids.extend(solid_collision_meshes(kernel, body, np.array(link['com'])))
grid = signed_distance_grid(meshes, .0005, solid_meshes=solids, bounds_m=[lo, hi], maximum_nodes=20000)
assert not parent.get('refinements'), 'existing local refinements require an explicit union/overlap plan'
parent['refinements'] = [{'grid': grid, 'blend_width_m': .001}]
recipe = {
    'version': 1, 'cad_sha256': scene['robot']['source']['cad_sha256'],
    'source_scene': str(source), 'source_scene_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
    'link': name, 'bounds_local_m': [lo.tolist(), hi.tolist()], 'cell_m': .0005,
    'blend_width_m': .001, 'maximum_nodes': 20000, 'actual_nodes': len(grid['values']),
    'tessellation_tolerance_mm': .15, 'wall_s': time.monotonic() - started,
    'provenance': 'Derived from versioned CAD B-rep, triangle distance and individual-solid union ray sign. Quintic blend to retained parent; approximate field, not exact CAD clearance. No source geometry or exclusion changes.',
    'cad_probe_local_m': points.tolist(),
}
previous = scene['robot']['source'].get('local_distance_refinements',[])
if not previous and scene['robot']['source'].get('local_distance_refinement'):
    previous = [scene['robot']['source']['local_distance_refinement']]
if previous:
    scene['robot']['source']['local_distance_refinements'] = previous + [recipe]
scene['robot']['source']['local_distance_refinement'] = recipe
(out / 'local-refinement.json').write_text(json.dumps(recipe, indent=2) + '\n')
(out / 'refined.scene.json').write_text(json.dumps(scene, separators=(',', ':')) + '\n')
print(json.dumps(recipe), flush=True)
