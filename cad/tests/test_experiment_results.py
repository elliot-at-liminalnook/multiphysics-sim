from copy import deepcopy
import json

import numpy as np
import pytest

from robocad.commands import Ops
from robocad.document import Document
from robocad.experiment_results import compare, evaluate_expectations, replay_flex, replay_matrices, signals, value_at
from robocad.kernel import KernelError
from robocad.physical import load_results
from robocad.snapshots import capture


def result():
    first = np.eye(4); second = np.eye(4); second[0, 3] = 10
    return {'run_id': 'a', 'trace': {'t': [0., .1, .2], 'joints': {'hinge': [0., .2, .4]},
        'poses': {'merged': [first.tolist(), second.tolist(), second.tolist()]}},
        'cad_mapping': [{'section': 'links', 'name': 'merged', 'id': 'p', 'members': ['p', 'fixed']},
            {'section': 'joints', 'name': 'hinge', 'id': 'j', 'related_ids': ['p', 'fixed']}],
        'controller_contract': {'actuators': [{'name': 'hinge.target', 'unit': 'rad'}]},
        'controller_frames': [{'t': 0., 'commands': {'hinge.target': 0.}}, {'t': .2, 'commands': {'hinge.target': .4}}],
        'settings': {'sample': .1}, 'provenance': {'source_hash': 'same', 'parameters_hash': 'same'}}


def test_units_expectations_and_missing_signals_fail_explicitly():
    r = result()
    metric = {'name': 'tracking', 'signal': 'joints/hinge/angle', 'unit': 'rad', 'reduction': 'rmse', 'target': .3, 'start': .1, 'max': .11}
    evaluation = evaluate_expectations(r, [metric])
    assert evaluation['status'] == 'passed'
    assert evaluation['metrics'][0]['value'] == pytest.approx(.1)
    assert evaluate_expectations(r, [{**metric, 'max': .09}])['status'] == 'failed'
    for bad in ({'unit': 'deg'}, {'signal': 'missing'}, {'start': 9}, {'max': float('nan')}, {'reduction': 'magic'}):
        with pytest.raises(KernelError): evaluate_expectations(r, [{**metric, **bad}])
    assert evaluate_expectations(r, [])['status'] == 'unchecked'


def test_comparison_resamples_and_holds_commands_without_extrapolating():
    a, b = result(), result(); b['run_id'] = 'b'
    b['trace']['t'] = [0., .05, .2]; b['trace']['joints']['hinge'] = [0., .1, .4]
    report = compare(a, b)
    assert report['signals']['joints/hinge/angle']['max_abs_delta'] == pytest.approx(0)
    assert report['same_scenario'] and report['same_settings']
    command = signals(a)['controller/commands/hinge.target']
    assert value_at(command, .15) == 0
    with pytest.raises(KernelError): value_at(command, -.01)
    b['settings']['sample'] = .05; b['provenance']['parameters_hash'] = 'new'
    assert len(compare(a, b)['caveats']) == 2


def test_objective_comparisons_require_matching_definitions_and_units():
    a, b = result(), result(); b['run_id'] = 'b'
    a['objectives'] = {'mass/moving': {'value': .3, 'unit': 'kg'}, 'tracking': {'value': .01, 'unit': 'rad', 'definition': {'start': .1}}}
    b['objectives'] = {'mass/moving': {'value': .2, 'unit': 'kg'}, 'tracking': {'value': .005, 'unit': 'rad', 'definition': {'start': .2}}}
    report = compare(a, b)
    assert report['objectives']['mass/moving']['delta'] == pytest.approx(-.1)
    assert not report['objectives']['tracking']['comparable']
    b['provenance']['seed'] = 17
    assert not compare(a, b)['same_scenario']


def test_replay_maps_fixed_members_and_part_filter_uses_stable_ids():
    r = result()
    matrices = replay_matrices(r, 1)
    assert matrices['p'][0, 3] == 10
    np.testing.assert_array_equal(matrices['p'], matrices['fixed'])
    assert set(signals(r)['joints/hinge/angle']['node_ids']) == {'j', 'p', 'fixed'}
    r['trace']['poses']['merged'].pop()
    with pytest.raises(KernelError, match='synchronized'): replay_matrices(r, 1)


def test_signal_declarations_use_full_component_names_and_captured_locations():
    r = {'trace': {'t': [0., 1.], 'signals': {
        'heater.node.temperature': [300., 301.],
        'heater.sensor.out.value': [300., 301.],
        'heater_other.out.value': [0., 0.],
        'graph/housing.node.temperature': [300., 301.]}},
        'script_component_mapping': [
            {'native_name': 'heater', 'source': 'system.rhai', 'line': 2, 'column': 5},
            {'native_name': 'heater.sensor', 'source': 'sensors.rhai', 'line': 7, 'column': 3}],
        'component_graph_mapping': [{'native_name': 'graph/housing', 'id': 'housing',
            'name': 'Motor housing', 'body_id': 'motor-body',
            'source': '__robocad_graph.rhai', 'line': 4}]}
    channels = signals(r)
    assert channels['heater.node.temperature']['source'] == {'path': 'system.rhai', 'line': 2, 'column': 5}
    assert channels['heater.sensor.out.value']['source'] == {'path': 'sensors.rhai', 'line': 7, 'column': 3}
    assert 'source' not in channels['heater_other.out.value']
    housing = channels['graph/housing.node.temperature']
    assert housing['source'] == {'path': '__robocad_graph.rhai', 'line': 4, 'column': 1}
    assert housing['component_id'] == 'housing' and housing['node_ids'] == ['motor-body']
    assert housing['identity'] == 'component/housing/node.temperature'


def test_flex_review_keeps_boundary_identity_units_and_display_scale_separate():
    a = result()
    a['trace']['flex'] = {'merged': [
        {'name': name, 'point_m': [[.1, 0, 0]]*3,
         'displacement_m': [[0, 0, d] for d in (0, -.00001*k, -.00002*k)]}
        for k, name in enumerate(('root', 'tip'), 1)]}
    channel = 'flex/merged/1:tip/dz'
    catalogue = signals(a)
    assert catalogue[channel]['unit'] == 'm'
    assert set(catalogue[channel]['node_ids']) == {'p', 'fixed'}
    assert catalogue[channel]['identity'] != catalogue['flex/merged/0:root/dz']['identity']
    arrows = replay_flex(a, 1, 100)
    assert arrows[1]['point_mm'] == [100, 0, 0]
    assert arrows[1]['tip_mm'] == pytest.approx([100, 0, -2])
    assert catalogue[channel]['values'][1] == -.00002
    b = deepcopy(a); b['run_id'] = 'b'
    b['cad_mapping'][0]['name'] = 'renamed'
    b['trace']['flex']['renamed'] = b['trace']['flex'].pop('merged')
    assert compare(a, b)['signals']['flex/renamed/1:tip/dz']['max_abs_delta'] == 0
    b['trace']['flex']['renamed'][1]['displacement_m'].pop()
    with pytest.raises(KernelError, match='synchronized'): signals(b)
    with pytest.raises(KernelError, match='synchronized'): replay_flex(b, 0)
    a['trace']['flex']['merged'][0]['point_m'][0] = [float('nan'), 0, 0]
    with pytest.raises(KernelError, match='Invalid flex vector'): signals(a)
    with pytest.raises(KernelError, match='Invalid flex vector'): replay_flex(a, 0)


def test_flex_comparison_tracks_attachment_ids_through_rename_and_reordering():
    a = result()
    a['trace']['flex'] = {'merged': [
        {'id': identity, 'name': identity, 'point_m': [[0, 0, 0]]*3, 'displacement_m': [[0, 0, d]]*3}
        for identity, d in [('joint-id', 0.), ('sensor-id', .001)]]}
    b = deepcopy(a); b['run_id'] = 'b'
    b['trace']['flex']['merged'].reverse()
    b['trace']['flex']['merged'][0]['name'] = 'renamed tip'
    compared = compare(a, b)['signals']['flex/merged/0:renamed tip/dz']
    assert compared['baseline_signal'] == 'flex/merged/1:sensor-id/dz'
    assert compared['max_abs_delta'] == 0
    b['trace']['flex']['merged'][0]['id'] = 'replacement'
    assert 'flex/merged/0:renamed tip/dz' not in compare(a, b)['signals']


def test_comparison_matches_renamed_joint_by_id_and_rejects_replacement():
    a, b = result(), result(); b['run_id'] = 'b'
    b['cad_mapping'][1]['name'] = 'renamed'
    b['trace']['joints']['renamed'] = b['trace']['joints'].pop('hinge')
    compared = compare(a, b)['signals']['joints/renamed/angle']
    assert compared['baseline_signal'] == 'joints/hinge/angle'
    assert compared['max_abs_delta'] == 0
    b['cad_mapping'][1]['id'] = 'replacement'
    assert 'joints/renamed/angle' not in compare(a, b)['signals']


def test_result_loading_retains_trace_and_maps_renamed_parts(tmp_path):
    doc = Document(); ops = Ops(doc)
    p = ops.box((0, 0, 0), (10, 10, 10), name='original')
    r = {'run_id': 'run', 'provenance': {'physical_hash': capture(doc).physical_hash},
         'trace': {'t': [0, 1]}, 'links': {'original': {'peak_stress_pa': 7}},
         'cad_mapping': [{'name': 'original', 'id': p, 'members': [p]}]}
    path = tmp_path/'result.json'; path.write_text(json.dumps(r))
    loaded = load_results(doc, str(path))
    assert loaded['trace'] == r['trace'] and not loaded['stale']
    ops.rename(p, 'renamed')
    assert doc.results['stale']
    load_results(doc, str(path))
    assert doc.nodes[p].results['peak_stress_pa'] == 7
    assert doc.results['stale']


def test_run_evidence_annotations_roundtrip_and_undo(tmp_path):
    doc = Document(); ops = Ops(doc)
    evidence = {'run_id': 'run', 'signal': 'joint/angle', 'time_range': [.1, .2],
                'source': {'path': 'system.rhai', 'line': 4, 'column': 2}}
    tid = ops.create_thread(body='Overshoot here', evidence=evidence, author='Agent')
    assert ops.thread(tid)['anchor_status'] == 'evidence'
    path = tmp_path/'annotated.rcad'; doc.save(str(path))
    assert Document.load(str(path)).annotations[tid]['evidence'] == evidence
    ops.undo(); assert not doc.annotations
    ops.redo(); assert doc.annotations[tid]['evidence'] == evidence
    with pytest.raises(KernelError): ops.update_thread(tid, evidence={**evidence, 'time_range': [1, 0]})
