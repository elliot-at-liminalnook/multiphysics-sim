"""Splitting for printing and the strength check, through the service the
REST API and the Print menu share."""
import math
import os

import numpy as np
import pytest

from robocad.api import Service
from robocad.commands import Ops
from robocad.document import Document
from robocad.print_split import SplitOptions, fits, split_for_printing
from robocad.printing import mesh_open_edges
from robocad import print_registry as pr
from robocad import print_study as ps

BIN = os.path.join(pr.REPO, 'target', 'release', 'sim-print')
TURNTABLE = os.path.join(pr.REPO, 'examples', 'camera-turntable', 'cad', 'turntable.rcad')


def bar_doc(length=250.0):
    doc = Document()
    ops = Ops(doc)
    bid = ops._new('bar', doc.kernel.box((0, 0, 0), (length, 60, 20)), 'Bar')
    return doc, ops, bid


def test_a_long_bar_splits_to_fit_with_pins_and_screws_and_undo_restores_it():
    doc, ops, bid = bar_doc()
    service = Service(doc, ops)
    out = service.print_request('POST', ['print', 'split'], {'node': bid, 'printer': 'bambu-a1-mini'})
    usable = pr.usable_mm('bambu-a1-mini')
    assert len(out['pieces']) == 2 and all(p['fits'] for p in out['pieces'])
    kinds = sorted(j['kind'] for s in out['seams'] for j in s['joints'])
    assert kinds.count('dowel') == 2 and kinds.count('insert_screw') >= 2, kinds
    hw = {h['item']: h['count'] for h in out['hardware']}
    assert hw['heat-set insert'] == hw['socket head screw'] == kinds.count('insert_screw')
    k = doc.kernel
    total = 0.0
    for pid in out['piece_nodes']:
        body = doc.nodes[pid].body
        assert mesh_open_edges(k.tessellate(body, 0.1)) == 0 and k.validate(body).valid
        lo, hi = k.bounding_box(body)
        assert fits(np.subtract(hi, lo), usable)
        assert doc.nodes[pid].robot['print_piece']['registry_sha256'] == pr.load()[1]
        total += k.mass_properties(body).volume
    # The pieces are the bar less the joint holes and screw pockets (a few percent).
    assert 0.9 * 250 * 60 * 20 < total < 250 * 60 * 20
    # The source is kept (hidden), untouched; one undo removes the split.
    assert not doc.nodes[bid].visible and abs(k.mass_properties(doc.nodes[bid].body).volume - 300000) < 1
    ops.undo()
    assert all(p not in doc.nodes for p in out['piece_nodes']) and doc.nodes[bid].visible


def test_a_bar_that_fits_is_not_split():
    doc, ops, bid = bar_doc(150.0)
    r = split_for_printing(doc, bid, SplitOptions(printer='bambu-a1-mini'))
    assert r.cuts == [] and len(r.pieces) == 1


def test_an_unknown_printer_is_refused_by_name():
    doc, ops, bid = bar_doc()
    from robocad.api import ApiError
    with pytest.raises(ApiError, match='not in the print registry'):
        Service(doc, ops).print_request('POST', ['print', 'split'], {'node': bid, 'printer': 'nope'})


@pytest.mark.skipif(not os.path.isfile(TURNTABLE), reason='turntable CAD missing')
def test_the_turntable_disc_splits_into_quarters_joined_by_jigsaw_tabs():
    doc = Document.load(TURNTABLE)
    disc = next(n.id for n in doc.bodies() if n.name.startswith('Turntable disc'))
    r = split_for_printing(doc, disc, SplitOptions(printer='bambu-a1-mini'))
    s = r.summary(doc.kernel)
    assert len(r.pieces) == 4 and all(p['fits'] for p in s['pieces'])
    assert len(r.seams) == 4
    for seam in r.seams:
        assert any(j.kind == 'dovetail' for j in seam.joints), seam.notes
        # Tabs drop in square to the plate.
        for j in seam.joints:
            if j.kind == 'dovetail':
                assert abs(abs(j.spec['along'][2]) - 1) < 1e-9


@pytest.mark.skipif(not os.path.isfile(BIN), reason='build sim-print first')
def test_a_strength_job_attaches_results_and_can_be_cancelled(tmp_path, monkeypatch):
    monkeypatch.setattr('robocad.print_jobs.RUNS', str(tmp_path))
    doc = Document()
    ops = Ops(doc)
    bid = ops._new('beam', doc.kernel.box((0, 0, 0), (100, 10, 10)), 'Beam')
    service = Service(doc, ops)
    body = {'parts': [{'node': bid, 'settings': {'walls': 3, 'infill': 1.0, 'layer_height': 0.2},
                       'fixtures': [{'name': 'root', 'region': {'below': {'axis': [1, 0, 0], 'height': 0.01}}}],
                       'loads': [{'name': 'tip', 'region': {'box': {'min': [99.99, -1, -1], 'max': [101, 11, 11]}}, 'direction': [0, 0, -1], 'magnitude': 10.0}]}],
            'voxel_mm': 1.0}
    job = service.print_request('POST', ['print', 'analyze'], body)
    done = service.print_jobs.wait(job['id'], 300)
    assert done['state'] == 'done', done['error']
    part = done['result']['parts'][0]
    # Root bending stress M·c/I = 6 MPa against 45 MPa along the layers (smoothed, so a little above 7.5).
    assert 6 < part['safety_factor'] < 12 and part['mode'] == 'in-layer'
    res = doc.nodes[bid].results
    assert res['section'] == 'print' and res['passes']
    field = ps.cached_field(res['result_dir'], res)
    vals = field.sample(np.array([[1.0, 5.0, 9.5], [90.0, 5.0, 5.0]]))
    assert vals[0] > vals[1] > 0 or (vals[0] > 0 and math.isfinite(vals[0]))
    # Cancelling a running job stops it.
    job2 = service.print_request('POST', ['print', 'analyze'], {**body, 'voxel_mm': 0.35})
    service.print_jobs.cancel(job2['id'])
    assert service.print_jobs.wait(job2['id'], 60)['state'] == 'cancelled'


@pytest.mark.skipif(not os.path.isfile(BIN), reason='build sim-print first')
def test_a_plan_job_picks_settings_and_writes_plates_with_per_object_settings(tmp_path, monkeypatch):
    import json
    import zipfile
    import xml.etree.ElementTree as ET
    monkeypatch.setattr('robocad.print_jobs.RUNS', str(tmp_path))
    doc = Document()
    ops = Ops(doc)
    bid = ops._new('beam', doc.kernel.box((0, 0, 0), (100, 10, 10)), 'Beam')
    service = Service(doc, ops)
    body = {'parts': [{'node': bid, 'directions': [[0, 0, 1], [1, 0, 0]],
                       'fixtures': [{'name': 'root', 'region': {'below': {'axis': [1, 0, 0], 'height': 0.01}}}],
                       'loads': [{'name': 'tip', 'region': {'box': {'min': [99.99, -1, -1], 'max': [101, 11, 11]}}, 'direction': [0, 0, -1], 'magnitude': 10.0}]}],
            'voxel_mm': 1.0, 'space': {'walls': [2, 3], 'infill': [0.15, 0.4], 'search_voxels': 3000}}
    job = service.print_request('POST', ['print', 'plan'], body)
    done = service.print_jobs.wait(job['id'], 600)
    assert done['state'] == 'done', done['error']
    r = done['result']
    chosen = r['parts'][0]['chosen']
    # Laid flat: the bending stress runs along the layers.
    assert chosen['build_direction'] == [0.0, 0.0, 1.0]
    assert r['parts'][0]['verified_safety_factor'] >= 2.0
    assert doc.nodes[bid].robot['print_plan']['settings'] == chosen['settings']
    plate = os.path.join(r['plates'], r['plate_files'][0])
    with zipfile.ZipFile(plate) as z:
        model = ET.fromstring(z.read('3D/3dmodel.model'))
        cfg = ET.fromstring(z.read('Metadata/model_settings.config'))
    assert len(model.findall('.//{*}build/{*}item')) == 1
    meta = {m.get('key'): m.get('value') for m in cfg.find('object').findall('metadata')}
    assert meta['wall_loops'] == str(chosen['settings']['walls'])
    assert meta['sparse_infill_density'] == f"{chosen['settings']['infill'] * 100:g}%"
    manifest = json.load(open(os.path.join(r['plates'], 'plates.json')))
    assert manifest['total_hours'] > 0 and manifest['registry_sha256'] == pr.load()[1]


def test_plates_pack_pieces_side_by_side_and_overflow_to_a_second_plate():
    from robocad.print_plan import pack
    doc = Document()
    k = doc.kernel
    pieces = [{'name': f'block {i}', 'body': k.box((0, 0, 0), (80, 50, 10)), 'build_direction': (0, 0, 1),
               'settings': {'walls': 3, 'infill': 0.15, 'layer_height': 0.2}, 'estimate': {'print_hours': 1.0}} for i in range(9)]
    plates = pack(k, pieces, 'bambu-a1-mini')
    assert len(plates) == 2 and sum(len(p.items) for p in plates) == 9
    ux, uy, _ = pr.usable_mm('bambu-a1-mini')
    for p in plates:
        for a in p.items:
            assert a.x + a.w <= ux + 1e-6 and a.y + a.d <= uy + 1e-6
            for b in p.items:
                if a is not b:
                    assert a.x + a.w <= b.x or b.x + b.w <= a.x or a.y + a.d <= b.y or b.y + b.d <= a.y


def test_junctions_are_found_where_a_post_rises_from_its_base_and_nowhere_else():
    from robocad.kernel.base import BooleanOp
    from robocad.print_strength_split import junction_planes
    doc = Document()
    k = doc.kernel
    stand = k.boolean(k.box((0, 0, 0), (100, 80, 8)), k.box((40, 30, 8), (20, 20, 200)), BooleanOp.UNION)
    planes = junction_planes(k, stand)
    assert len(planes) == 1, planes
    p = planes[0]
    # Just above the base, facing up into the post.
    assert p['normal'] == [0.0, 0.0, 1.0] and 9.0 < p['point'][2] < 12.0


def test_an_assembly_guide_covers_every_piece_and_its_hardware(tmp_path):
    from robocad.print_assembly import add_exploded_view, plan_assembly, write_guide
    doc, ops, bid = bar_doc(400.0)
    group = ops.print_split(bid, printer='bambu-a1-mini')
    plan = plan_assembly(doc, group)
    pieces = len(doc.nodes[group].children)
    assert pieces == 3
    added = [s['piece'] for s in plan['steps'] if s['title'].startswith(('Start', 'Add'))]
    assert sorted(added) == list(range(pieces))
    # Every insert, screw and pin in the hardware list appears in some step.
    need = {(h['item'], h['size']): h['count'] for h in plan['hardware']}
    used: dict = {}
    for s in plan['steps']:
        for h in s['hardware']:
            key = (h['item'], h['size']) if h['item'] != 'steel dowel pin' else ('steel dowel pin', h['size'])
            used[key] = used.get(key, 0) + h['count']
    assert sum(used.values()) == sum(need.values()), (used, need)
    assert any('hex key' in t for t in plan['tools'])
    guide = write_guide(doc, plan, str(tmp_path), images=False)
    assert os.path.getsize(guide) > 1000
    g = add_exploded_view(ops, plan)
    moved = [doc.nodes[i].transform.translation for i in doc.nodes[g].children]
    assert sum(1 for t in moved if any(abs(x) > 1 for x in t)) == pieces - 1
    ops.undo()
    assert g not in doc.nodes


@pytest.mark.skipif(not os.path.isfile(BIN), reason='build sim-print first')
def test_coupons_print_on_a_plate_and_their_results_promote(tmp_path):
    import json
    import subprocess
    import zipfile
    import xml.etree.ElementTree as ET
    from robocad.print_coupons import write_coupon_kit
    doc, ops, bid = bar_doc()
    group = ops.print_split(bid, printer='bambu-a1-mini')
    split = doc.nodes[group].robot['print_split']
    kit = write_coupon_kit(doc.kernel, str(tmp_path), 'pla-basic', 'bambu-h2c', split)
    kinds = {c['kind'] for c in kit['coupons']}
    assert kinds == {'tensile_in_layer', 'tensile_across_layers', 'pin_shear', 'insert_pullout', 'dovetail_pull'}
    # The pin coupon copies the split's pin.
    pin = next(c for c in kit['coupons'] if c['kind'] == 'pin_shear')
    seam_pin = next(j for s in split['seams'] for j in s['joints'] if j['kind'] == 'dowel')
    assert pin['geometry']['pin_diameter_mm'] == seam_pin['spec']['diameter_mm']
    n = 0
    for plate in kit['plates']:
        with zipfile.ZipFile(tmp_path / plate) as z:
            n += len(ET.fromstring(z.read('3D/3dmodel.model')).findall('.//{*}build/{*}item'))
    assert n == len(kit['coupons'])
    # Fill the template and promote into a copy of the registry.
    results = json.load(open(kit['results_template']))
    for t in results['tests']:
        t['failure_n'] = [1000.0, 1050.0, 980.0]
    filled = tmp_path / 'filled.json'
    filled.write_text(json.dumps(results))
    reg_copy = tmp_path / 'registry.json'
    reg_copy.write_text(open(pr.default_path()).read())
    out = subprocess.run([BIN, 'promote', str(filled), '--registry', str(reg_copy), '--write'], capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    report = json.loads(out.stdout)
    assert len(report['promotions']) == 5 and report['written']
    new = json.loads(reg_copy.read_text())
    assert new['revision'] == json.load(open(pr.default_path()))['revision'] + 1
    assert new['materials']['pla-basic']['tensile_in_layer']['provenance'] == 'measured'
