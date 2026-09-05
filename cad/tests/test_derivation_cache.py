import json
import pytest
from robocad.commands import Ops
from robocad.document import Document
from robocad.derivation_cache import DerivationCache
from robocad.physical import export_physical_model
from robocad.kernel import KernelError


def physical(model):
    return {k: v for k, v in model.items() if k != 'source'}


def equivalent(a, b):
    if isinstance(a, dict):
        assert a.keys() == b.keys()
        for key in a: equivalent(a[key], b[key])
    elif isinstance(a, list):
        assert len(a) == len(b)
        for left, right in zip(a, b): equivalent(left, right)
    elif type(a) in (float, int): assert a == pytest.approx(b, rel=1e-12, abs=1e-12)
    else: assert a == b


def test_part_geometry_and_material_edits_reuse_only_matching_dependencies(tmp_path):
    doc = Document(); ops = Ops(doc)
    a = ops.box((0, 0, 0), (10, 10, 20))
    b = ops.box((30, 0, 0), (12, 10, 20))
    def export():
        cache = DerivationCache(tmp_path, {'version': 'test'})
        return export_physical_model(doc, flex=False, cache=cache), cache.stats
    cold, first = export()
    assert first['body_properties']['misses'] == 2 and first['collision']['misses'] == 2
    cached, reuse = export()
    assert physical(cold) == physical(cached)
    assert reuse['body_properties']['hits'] == 2 and reuse['collision']['hits'] == 2
    ops.transform([a], scale=1.1)
    changed, stats = export()
    assert stats['body_properties']['hits'] == stats['body_properties']['misses'] == 1
    assert stats['collision']['hits'] == stats['collision']['misses'] == 1
    assert next(l for l in changed['links'] if l['id'] == a)['mass'] > next(l for l in cold['links'] if l['id'] == a)['mass']
    assert next(l for l in changed['links'] if l['id'] == b) == next(l for l in cold['links'] if l['id'] == b)
    ops.set_material([a], 'steel')
    material, stats = export()
    assert stats['body_properties']['hits'] == 2 and stats['body_properties']['misses'] == 0
    assert next(l for l in material['links'] if l['id'] == a)['mass'] > next(l for l in changed['links'] if l['id'] == a)['mass']
    # Compare against the uncached exporter after both kinds of input edits.
    # Repeated OCCT mass queries may differ in their final floating-point bits.
    equivalent(physical(material), physical(export_physical_model(doc, flex=False)))
    # Cached contents are checked, not trusted merely because the key exists.
    for path in tmp_path.glob('*/body_properties/*.json'):
        entry = json.loads(path.read_text()); entry['value']['volume'] = 999999.; path.write_text(json.dumps(entry))
    repaired, stats = export()
    assert stats['body_properties']['misses'] == 2
    equivalent(physical(repaired), physical(material))


def test_joint_edits_and_overrides_do_not_reuse_stale_physics(tmp_path):
    doc = Document(); ops = Ops(doc)
    base = ops.box((-10, -10, -10), (20, 20, 10))
    arm = ops.box((-5, -5, 0), (10, 10, 60))
    joint = ops.add_joint('revolute', base, arm, (0, 0, 0), (0, 1, 0))
    def export():
        cache = DerivationCache(tmp_path, {'version': 'test'})
        return export_physical_model(doc, flex=False, cache=cache), cache.stats
    original, _ = export()
    ops.set_joint(joint, pivot=(0, 0, 10))
    moved, stats = export()
    assert stats['joint']['misses'] == 1
    assert stats['collision']['hits'] == stats['body_properties']['hits'] == 2
    assert moved['joints'][0]['origin'] == [0, 0, .01]
    assert moved['joints'][0]['physics']['lever'] != original['joints'][0]['physics']['lever']
    ops.set_joint_physics(joint, friction={'coulomb': .123}, hole_radius=.006)
    overridden, stats = export()
    assert overridden['joints'][0]['physics']['friction']['coulomb'] == .123
    assert overridden['joints'][0]['physics']['hole_radius'] == .006
    # Overrides are applied after the reusable inferred physics is read.
    assert stats['joint']['hits'] == 1
    equivalent(physical(overridden), physical(export_physical_model(doc, flex=False)))
    ops.stack.undo()
    reverted, _ = export()
    assert physical(reverted) == physical(moved)


def test_added_fastener_recomputes_attachment_and_merged_geometry(tmp_path):
    from robocad.printing import FastenerSpec
    doc = Document(); ops = Ops(doc)
    base = ops.box((0, 0, 0), (40, 40, 6), name='base')
    bracket = ops.box((10, 10, 6), (20, 20, 6), name='bracket')
    ops.connect_fixed(base, bracket)
    def add_hole(point):
        face = next(f for f in doc.kernel.faces(doc.nodes[bracket].body) if f.normal[2] > .9)
        ops.fastener_hole(bracket, face, point, FastenerSpec('M3', 'clearance'))
    def export():
        cache = DerivationCache(tmp_path, {'version': 'test'})
        return export_physical_model(doc, flex=False, cache=cache), cache.stats
    add_hole((15, 15, 12))
    original, _ = export()
    repeat, stats = export()
    assert stats['attachment']['hits'] == 1
    assert physical(original) == physical(repeat)
    add_hole((25, 25, 12))
    changed, stats = export()
    assert stats['attachment']['misses'] == stats['collision']['misses'] == 1
    assert stats['body_properties']['hits'] == stats['body_properties']['misses'] == 1
    before, after = original['joints'][0]['fastened'], changed['joints'][0]['fastened']
    assert before['count'] == 1 and after['count'] == 2
    assert after['stiffness'] == pytest.approx(2 * before['stiffness'])
    equivalent(physical(changed), physical(export_physical_model(doc, flex=False)))


def test_sensor_attachment_invalidates_flex_boundary_mapping(tmp_path):
    doc = Document(); ops = Ops(doc)
    base = ops.box((-10, -10, -10), (10, 20, 20)); ops.set_material([base], 'al')
    beam = ops.box((0, -5, -5), (100, 10, 10)); ops.set_material([beam], 'pla')
    joint = ops.add_joint('revolute', base, beam, (0, 0, 0), (0, 1, 0), name='root')
    def export():
        cache = DerivationCache(tmp_path, {'version': 'test'})
        model = export_physical_model(doc, flex=True, cache=cache)
        link = next(l for l in model['links'] if l['id'] == beam)
        assert link['flex'], link.get('flex_error')
        return model, link['flex'], cache.stats
    original, before, _ = export()
    repeated, _, stats = export()
    assert stats['flex']['hits'] == 1
    assert physical(repeated) == physical(original)
    ops.add_sensor('imu', beam, (90, 0, 0), name='tip sensor')
    changed, after, stats = export()
    assert stats['flex']['misses'] == 1
    assert stats['collision']['hits'] == stats['body_properties']['hits'] == 2
    assert len(after['boundary_frames']) == len(before['boundary_frames']) + 1
    assert any(f['name'] == 'tip sensor' for f in after['boundary_frames'])
    # Rebuilding modal bases can flip eigenvector signs; compare the physical
    # invariants and boundary identities, not arbitrary basis signs.
    uncached = export_physical_model(doc, flex=True)
    fresh = next(l['flex'] for l in uncached['links'] if l['id'] == beam)
    assert after['frequencies_hz'] == pytest.approx(fresh['frequencies_hz'], rel=1e-8)
    assert after['gravity_sag_m'] == pytest.approx(fresh['gravity_sag_m'], rel=1e-8)
    assert after['boundary_frames'] == fresh['boundary_frames']
    ops.set_joint_physics(joint, flex_patch_radius=.008)
    _, clamped, stats = export()
    assert stats['flex']['misses'] == 1
    assert stats['collision']['hits'] == stats['body_properties']['hits'] == 2
    patch = next(f['patch'] for f in clamped['boundary_frames'] if f['id'] == joint)
    assert patch['radius_m'] == .008 and patch['radius_source'] == 'declared'
    assert clamped['gravity_sag_m'] < .9*after['gravity_sag_m']
    ops.undo()
    _, restored, stats = export()
    assert stats['flex']['hits'] == 1 and restored == after
    revision = doc.revision
    for bad in (0, -.001, float('nan'), float('inf'), True, '8 mm'):
        with pytest.raises(KernelError, match='flex_patch_radius'):
            ops.set_joint_physics(joint, flex_patch_radius=bad)
        assert doc.revision == revision
