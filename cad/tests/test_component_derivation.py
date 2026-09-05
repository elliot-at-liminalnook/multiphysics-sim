import math
from copy import deepcopy
import pytest
from robocad.commands import Ops
from robocad.document import Document
from robocad.component_graph import empty_graph, validate_graph
from robocad.component_derivation import derive_graph
from robocad.derivation_cache import DerivationCache
from robocad.snapshots import capture
from robocad.kernel import KernelError


def graph_for(body, kind, native):
    graph = empty_graph()
    graph['components']['derived'] = {'id': 'derived', 'name': 'Derived component', 'body_id': body,
        'type': native, 'parameters': {}, 'derivation': {'kind': kind}}
    return graph


def test_thermal_capacity_tracks_geometry_material_and_explicit_recipe(tmp_path):
    doc = Document(); ops = Ops(doc)
    body = ops.box((0, 0, 0), (10, 20, 30)); ops.set_material([body], 'al')
    graph = graph_for(body, 'body_thermal_capacity', 'thermal.capacitance')
    ops.set_component_graph(graph)
    first = capture(doc)
    def derive():
        cache = DerivationCache(tmp_path, {'test': 1})
        result, evidence = derive_graph(doc.component_graph, doc, cache)
        return result['components']['derived']['parameters']['heat_capacity'], evidence[0], cache.stats
    capacity, evidence, _ = derive()
    assert capacity == pytest.approx(.0162*896., rel=1e-8)
    assert evidence['inputs']['mass_kg'] == pytest.approx(.0162)
    assert evidence['outputs']['heat_capacity']['unit'] == 'J/K'
    assert doc.component_graph == graph  # Resolving cannot rewrite authored inputs.
    repeat, _, stats = derive()
    assert capacity == repeat and stats['body_properties']['hits'] == 1
    changed = deepcopy(graph); changed['components']['derived']['derivation']['specific_heat'] = 1000.
    ops.set_component_graph(changed)
    assert capture(doc).physical_hash != first.physical_hash
    assert capture(doc).cad_derivation_hash == first.cad_derivation_hash
    overridden, evidence, stats = derive()
    assert overridden == pytest.approx(.0162*1000.)
    assert evidence['inputs']['specific_heat_source'] == 'recipe'
    assert stats['body_properties']['hits'] == 1
    ops.transform([body], scale=2.)
    scaled, _, stats = derive()
    assert scaled == pytest.approx(overridden*8.)
    assert stats['body_properties']['misses'] == 1
    assert capture(doc).cad_derivation_hash != first.cad_derivation_hash


def test_circular_fluid_volume_derives_si_pipe_dimensions_and_orientation(tmp_path):
    doc = Document(); ops = Ops(doc)
    body = ops.cylinder((0, 0, 10), (0, 0, 1), 5., 100.)
    graph = graph_for(body, 'circular_fluid_volume', 'fluid.pipe_ph')
    cache = DerivationCache(tmp_path, {'test': 1})
    resolved, evidence = derive_graph(graph, doc, cache)
    assert resolved['components']['derived']['parameters'] == pytest.approx({'length': .1, 'diameter': .01, 'rise': .1})
    assert evidence[0]['inputs']['area_m2'] == pytest.approx(math.pi*.005**2)
    graph['components']['derived']['derivation']['flow_direction'] = -1
    reversed_graph, reversed_evidence = derive_graph(graph, doc, cache)
    assert reversed_graph['components']['derived']['parameters']['rise'] == pytest.approx(-.1)
    assert reversed_evidence[0]['inputs']['end_a_mm'] == evidence[0]['inputs']['end_b_mm']
    assert cache.stats['body_faces']['hits'] == 1


def test_recipes_reject_ambiguous_parameters_and_nonfluid_geometry():
    doc = Document(); ops = Ops(doc)
    body = ops.box((0, 0, 0), (10, 10, 100))
    graph = graph_for(body, 'circular_fluid_volume', 'fluid.pipe_ph')
    with pytest.raises(KernelError, match='closed circular cylinder'):
        derive_graph(graph, doc)
    graph = graph_for(body, 'body_thermal_capacity', 'thermal.capacitance')
    graph['components']['derived']['parameters']['heat_capacity'] = 1.
    with pytest.raises(KernelError, match='also have explicit values'): validate_graph(graph, doc)
    graph['components']['derived']['parameters'] = {}
    graph['components']['derived']['derivation']['specific_heat'] = 0.
    with pytest.raises(KernelError, match='specific_heat'): validate_graph(graph, doc)
