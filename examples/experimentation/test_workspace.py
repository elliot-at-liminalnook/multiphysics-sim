"""Real CAD → captured Rhai → Rust → measured/replayed result acceptance."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import time

import numpy as np
import pytest

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT/'cad'))
from robocad.commands import Ops
from robocad.experiment_results import replay_flex, replay_matrices, signals
from robocad.experiments import Experiments, TERMINAL
from robocad.snapshots import capture


@pytest.fixture(scope='module')
def builder():
    binary = ROOT/'target'/'release'/('sim-experiment.exe' if os.name == 'nt' else 'sim-experiment')
    if not binary.exists():
        if os.environ.get('SIM_EXPERIMENT_REQUIRED'): pytest.fail('Build sim-experiment before workspace acceptance')
        pytest.skip('Build sim-experiment to run workspace acceptance')
    spec = importlib.util.spec_from_file_location('workspace_build_model', HERE/'build_model.py')
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module.build


def request(doc):
    files = {p.name: p.read_text() for p in HERE.glob('*.rhai')}
    return {'expected_revision': doc.revision, 'parameters': {'target1': .2},
            'system': {'entry': 'system.rhai', 'files': files},
            'controller': {'language': 'rhai', 'sources': {'entry': 'controller.rhai', 'files': files},
                           'parameters': {'target1': .2, 'target2': -.15}}}


def wait(manager, job, process_events=lambda: None, timeout=60):
    start = time.monotonic(); gaps = []; previous = start
    while manager.get(job['id'])['state'] not in TERMINAL:
        process_events()
        current = time.monotonic(); gaps.append(current-previous); previous = current
        if current-start > timeout:
            manager.cancel(job['id']); pytest.fail(f"Run timed out: {manager.get(job['id'])}")
        time.sleep(.005)
    return manager.get(job['id']), max(gaps, default=0.)


def test_cad_free_fall_refines_and_box_contact_settles_at_explicit_floor(builder, tmp_path):
    from robocad.document import Document
    doc = Document(); ops = Ops(doc)
    body_id = ops.box((-25, -25, 300), (50, 50, 50), name='falling box')
    ops.set_material([body_id], 'pla')
    ops.set_robot_setting('world', {'floor_z': 0., 'floor_stiffness': 2e5, 'floor_damping': 2e3})
    manager = Experiments(doc, root=tmp_path)
    free_errors = []; contact_traces = []
    try:
        for contact, step in [(False, .0005), (False, .00025), (True, .0005), (True, .0005)]:
            job = manager.create({'expected_revision': doc.revision,
                'system': 'let assembly = cad("assembly");', 'controller': None,
                'settings': {'contact': contact, 'flex': False, 'noise': False,
                             'seconds': 1., 'step': step, 'sample': .0025}})
            record, _ = wait(manager, job)
            assert record['state'] == 'completed', manager.diagnostics(job['id'])
            result = manager.result(job['id']); trace = result['trace']
            assert any(body_id in item['members'] for item in result['cad_mapping'])
            assert len(trace['poses']) == 1
            t = np.asarray(trace['t'])
            dz = np.asarray(next(iter(trace['poses'].values())))[:, 2, 3] / 1000.
            assert abs(t[-1] - 1.) < 1e-10
            # CAD uses backward Euler: constant gravity gives z=-g*t*(t+h)/2.
            falling = t < .15 if contact else np.ones(t.shape, dtype=bool)
            np.testing.assert_allclose(dz[falling], -.5*9.81*t[falling]*(t[falling]+step), atol=1e-8, rtol=0)
            if not contact:
                free_errors.append(abs(dz[-1] + .5*9.81*t[-1]**2))
            else:
                # This 50-mm PLA box starts with its bottom 300 mm above z=0.
                assert abs(dz[-1] + .300) < 5e-6  # settled position: 5 micrometres
                assert dz.min() > -.3005  # impact penetration: at most 0.5 mm
                assert np.ptp(dz[t > .8]) < 1e-6
                contact_traces.append(trace)
        assert 1.99 < free_errors[0] / free_errors[1] < 2.01
        assert contact_traces[0] == contact_traces[1]
        assert result['cache']['cad_hit']
    finally:
        manager.close()


@pytest.mark.parametrize('patch_radius', [None, .008])
def test_cad_flex_boundary_trace_matches_modal_equilibrium_and_cached_replay(builder, tmp_path, patch_radius):
    from robocad.document import Document
    doc = Document(); ops = Ops(doc)
    wall = ops.box((-10, -10, -10), (10, 20, 20), name='wall')
    ops.set_material([wall], 'al'); ops.set_ground(wall)
    beam = ops.box((0, -5, -5), (100, 10, 10), name='beam')
    ops.set_material([beam], 'pla')
    ops.set_material_props('pla', **{'print': {'anisotropy_z': 1.0}})
    # Root permits unloaded roll, but gravity bending is clamped at its patch.
    root = ops.add_joint('revolute', wall, beam, (0, 0, 0), (1, 0, 0), name='root')
    sensor = ops.add_sensor('imu', beam, (100, 0, 0), name='tip')
    if patch_radius is not None: ops.set_joint_physics(root, flex_patch_radius=patch_radius)
    ops.set_robot_setting('world', {'ambient_c': 20.})
    manager = Experiments(doc, root=tmp_path)
    traces = []
    try:
        for _ in range(2):
            job = manager.create({'expected_revision': doc.revision, 'controller': None,
                'system': 'let assembly=cad("assembly"); let plant=bind_component("plant","robot","robot.articulated",#{});',
                'settings': {'flex': True, 'contact': False, 'noise': False,
                             'seconds': .2, 'step': .0001, 'sample': .001}})
            record, _ = wait(manager, job)
            assert record['state'] == 'completed', manager.diagnostics(job['id'])
            result = manager.result(job['id']); traces.append(result['trace'])
            physical = json.loads((tmp_path/job['id']/'physical.json').read_text())
            link = next(l for l in physical['links'] if beam in l['members'])
            flex = link['flex']; catalogue = signals(result)
            assert flex['normalization'] == 'mass_normalized'
            boundary = next(f for f in flex['boundary_frames'] if f['id'] == root)
            assert boundary['patch']['radius_m'] == pytest.approx(patch_radius or .00455)
            assert boundary['patch']['radius_source'] == ('inferred' if patch_radius is None else 'declared')
            assert boundary['patch']['selection'] == 'within_radius'
            if patch_radius is not None:
                # Captured isotropic, homogenized modulus and exact CAD mass;
                # this declared 8-mm patch approximates the clamped beam face.
                E = flex['fe']['modulus']; density = link['mass']/(.1*.01*.01)
                area, inertia, length = .01**2, .01**4/12, .1
                frequency = 1.875104**2/(2*np.pi)*np.sqrt(E*inertia/(density*area*length**4))
                sag = density*area*9.81*length**4/(8*E*inertia)
                assert abs(flex['frequencies_hz'][0]/frequency-1) < .15
                assert abs(flex['gravity_sag_m']/sag-1) < .15
            modes = sorted(k for k in catalogue if k.startswith('plant.beam.eta') and not k.startswith('plant.beam.etad'))
            assert modes and all(catalogue[k]['unit'] == 'm·√kg' for k in modes)
            tip = next(i for i, frame in enumerate(flex['boundary_frames']) if frame['name'] == 'tip')
            shapes = np.asarray(flex['boundary_shapes'])[:len(modes), tip, :3]
            reconstructed = shapes.T @ np.asarray([catalogue[k]['values'] for k in modes])
            measured = np.asarray(result['trace']['flex']['beam'][tip]['displacement_m']).T
            np.testing.assert_allclose(measured, reconstructed, atol=1e-10, rtol=1e-5)
            soft = flex['softening']; temperature = physical['world']['ambient_c']
            factor = soft['ratio_above'] + (1-soft['ratio_above'])/(1+np.exp((temperature-soft['tg_c'])/soft['width_c']))
            loads = np.asarray(flex['participation'])[:len(modes), :3] @ np.asarray(physical['gravity'])
            equilibrium = shapes.T @ (loads/(np.asarray(flex['modal_stiffness'])[:len(modes)]*factor))
            # Solver/reduction contract for this captured model, not a claim of
            # continuum accuracy for the voxel mesh or the finite root patch.
            np.testing.assert_allclose(measured[:, -1], equilibrium, atol=1e-9, rtol=1e-3)
            dz = catalogue[f'flex/beam/{tip}:tip/dz']
            assert dz['unit'] == 'm' and beam in dz['node_ids']
            assert dz['identity'].endswith('/flex/'+sensor)
            arrow = next(a for a in replay_flex(result, len(result['trace']['t'])-1, 100) if a['name'] == 'beam/tip')
            np.testing.assert_allclose(np.asarray(arrow['tip_mm'])-arrow['point_mm'], measured[:, -1]*100000, atol=1e-10)
        assert traces[0] == traces[1]
        assert result['cache']['cad_hit']
        if patch_radius is not None:
            invalid = manager.create({'expected_revision': doc.revision, 'controller': None,
                'preflight': True, 'system': 'let assembly=cad("assembly");\n'
                    'configure(#{cad_overrides:[#{section:"joints",id:"root",field:"/physics/flex_patch_radius",value:0.01}]});'})
            record, _ = wait(manager, invalid)
            assert record['state'] == 'failed'
            assert 'before CAD derivation' in record['error'] and 'system.rhai:2:' in record['error']
            # A root patch covering the entire mesh leaves no nodes for the tip
            # patch. Reduction failure must not silently become a rigid run.
            ops.set_joint_physics(root, flex_patch_radius=1.)
            failed_request = {'expected_revision': doc.revision, 'controller': None, 'preflight': True,
                'system': 'let assembly=cad("assembly");', 'settings': {'flex': True}}
            invalid = manager.create(failed_request); record, _ = wait(manager, invalid)
            assert record['state'] == 'failed'
            assert 'Flex derivation failed' in record['error'] and beam in record['error']
            failed_model = json.loads((tmp_path/invalid['id']/'physical.json').read_text())
            failed_link = next(l for l in failed_model['links'] if beam in l['members'])
            assert 'fewer than four available mesh nodes' in failed_link['flex_error']
            rigid = manager.create({**failed_request, 'settings': {'flex': False}})
            record, _ = wait(manager, rigid)
            assert record['state'] == 'completed', manager.diagnostics(rigid['id'])
    finally:
        manager.close()


def test_graph_edits_reuse_mechanical_cad_and_record_attached_temperature(builder, tmp_path):
    from robocad.component_graph import empty_graph
    doc, ids = builder(False); ops = Ops(doc)
    graph = empty_graph()
    graph['components'] = {
        'housing': {'id': 'housing', 'name': 'Housing', 'body_id': ids['first'], 'type': 'thermal.capacitance',
                    'parameters': {'heat_capacity': 20., 'initial.temperature': 300.}},
        'heat': {'id': 'heat', 'name': 'Losses', 'type': 'thermal.heat_source', 'parameters': {'power': 10.}},
    }
    graph['connections']['thermal'] = {'id': 'thermal', 'ports': [
        {'component_id': 'housing', 'port': 'node'}, {'component_id': 'heat', 'port': 'node'}]}
    ops.set_component_graph(graph)
    manager = Experiments(doc, root=tmp_path)
    try:
        for power in (10., 100.):
            graph['components']['heat']['parameters']['power'] = power; ops.set_component_graph(graph)
            job = manager.create({'expected_revision': doc.revision, 'system': 'let assembly = cad("assembly");',
                'controller': None, 'settings': {'seconds': .1, 'step': .001, 'sample': .01}})
            record, _ = wait(manager, job)
            assert record['state'] == 'completed', manager.diagnostics(job['id'])
            result = manager.result(job['id'])
            assert result['cache']['cad_hit'] == (power == 100.)
            temperature = signals(result)['graph/housing.node.temperature']
            assert temperature['node_ids'] == [ids['first']]
            assert temperature['component_id'] == 'housing'
            np.testing.assert_allclose(temperature['values'], 300.+power/20.*np.asarray(temperature['t']), atol=1e-8)
            assert len(result['trace']['poses']) > 0
    finally:
        manager.close()


def test_preflight_discovers_native_components_without_sampling_and_retains_failed_bindings(builder, tmp_path):
    from robocad.api import ApiServer
    from robocad.client import RoboClient
    from robocad.component_graph import empty_graph
    from robocad.kernel import KernelError
    doc, ids = builder(False); manager = Experiments(doc, root=tmp_path)
    server = ApiServer(doc, port=0); server.service._experiments = manager; server.start()
    client = RoboClient(server.url)
    req = {'expected_revision': doc.revision, 'system': '''let assembly = cad("assembly");
        configure(#{expectations:[#{name:"Not measured",signal:"absent",unit:"K",max:0.0}]});''',
        'controller': {'language': 'rhai', 'sources': 'fn control(t,s,a,state) { throw "must not be sampled"; }'}}
    try:
        checked = client.check_system(req); record, _ = wait(manager, checked)
        assert record['state'] == 'completed', manager.diagnostics(checked['id'])
        result = manager.result(checked['id'])
        assert result['preflight'] and result['duration_s'] == 0
        assert result['trace'] == {} and result['controller_frames'] == []
        assert result['evaluation'] == {'status': 'not_simulated', 'metrics': []}
        assert result['timing']['step_s'] == 0 and record['stage'] == 'checked'
        discovered = client.experiment_components(checked['id'])
        assert not discovered['stale'] and discovered['resolved']
        case = next(c for c in discovered['imported'] if c['name'] == 'drive1.case')
        assert case['body_id'] in doc.nodes and case['binding'] == f"cad/{case['body_id']}/case"
        assert case['parameters']['heat_capacity'] > 0
        assert case['ports'][0]['lanes'][0]['across_unit'] == 'K'
        with pytest.raises(KernelError, match='Build-only'):
            manager.compare(checked['id'], checked['id'])
        graph = empty_graph()
        graph['components']['case'] = {'id': 'case', 'name': 'Broken binding', 'type': 'thermal.capacitance',
            'body_id': case['body_id'], 'binding': f"cad/{case['body_id']}/missing", 'parameters': {}}
        client.set_system_graph(graph, doc.revision)
        failed = client.check_system({**req, 'expected_revision': doc.revision})
        record, _ = wait(manager, failed)
        assert record['state'] == 'failed' and '__robocad_graph.rhai:' in record['error']
        retained = client.experiment_components(failed['id'])
        assert retained['resolved'] is None and retained['error']
        assert case in retained['imported']
        assert client.experiment_components(checked['id'])['stale']
        # Source contents and available values belong to each check's capture.
        assert manager.inputs(checked['id'])['component_graph'] == empty_graph()
        client.update_system_component('case', {'binding': case['binding'],
            'parameters': {'initial.node.temperature': case['parameters']['initial.temperature'] + 10.}}, doc.revision)
        conflict = client.check_system({**req, 'expected_revision': doc.revision})
        record, _ = wait(manager, conflict)
        assert record['state'] == 'failed'
        for expected in ['conflicting initial values', 'drive1.case.initial.temperature', 'drive1.case.initial.node.temperature',
            '[K]', '__robocad_graph.rhai:']:
            assert expected in record['error'], record['error']
    finally:
        server.stop(); manager.close()


def test_preflight_picker_adds_a_body_bound_component_through_the_ui(builder, tmp_path, monkeypatch):
    from functools import partial
    from PySide6.QtCore import QCoreApplication, QEvent
    from PySide6.QtTest import QTest
    from PySide6.QtWidgets import QApplication, QPushButton
    from robocad.document import Document
    import robocad.ui.app as ui
    application = QApplication.instance() or QApplication([])
    monkeypatch.setattr(ui.MainWindow, 'start_api', lambda self: None)
    monkeypatch.setattr(Document, 'start_autosave', lambda *args: None)
    monkeypatch.setattr(ui, 'Experiments', partial(Experiments, root=tmp_path))
    doc, _ = builder(False); window = ui.MainWindow(doc)
    try:
        window.resize(1500, 1050); window.show(); window.experiments_dock.show(); window.experiments_dock.raise_()
        panel = window.experiments_panel; graph = panel.graph_panel
        panel.tabs.setCurrentWidget(graph)
        graph.receive_catalogue(window.experiments.catalogue())
        panel.findChild(QPushButton, 'checkSystem').click()
        checked_id = panel.current_id()
        record, _ = wait(window.experiments, {'id': checked_id}, application.processEvents)
        assert record['state'] == 'completed', window.experiments.diagnostics(checked_id)
        QTest.qWait(20)
        assert not panel.baseline.isEnabled() and not panel.inspect.isEnabled()
        assert 'No simulation samples' in panel.diagnostics.toPlainText()
        index = next(i for i in range(graph.imports.count()) if graph.imports.itemData(i)['name'] == 'drive1.case')
        entry = graph.imports.itemData(index); graph.imports.setCurrentIndex(index); graph.use_import.click()
        assert graph.binding.text() == entry['binding'] and graph.body.currentData() == entry['body_id']
        row = next(i for i in range(graph.parameters.rowCount()) if graph.parameters.item(i, 0).text() == 'heat_capacity')
        assert 'captured value:' in graph.parameters.item(row, 1).toolTip()
        assert graph.parameters.item(row, 1).text() == ''
        assert float(graph.parameters.item(row, 3).text()) == pytest.approx(entry['parameters']['heat_capacity'])
        graph.derivation.setCurrentIndex(graph.derivation.findData('body_thermal_capacity'))
        graph.apply_button.click()
        component_id = graph.current
        assert doc.component_graph['components'][component_id]['binding'] == entry['binding']
        assert 'document has changed' in graph.import_status.text()
        graph.use_import.click()
        assert graph.current == component_id and len(doc.component_graph['components']) == 1
        window.viewport.focus_all(); application.processEvents()
        graph.grab().save(str(tmp_path/'preflight-inspector.png'))
        panel.parameters.setPlainText(json.dumps({'controller': {'target': .2}, 'settings': {'seconds': .05}}))
        job = panel.run(); record, _ = wait(window.experiments, job, application.processEvents)
        assert record['state'] == 'completed', window.experiments.diagnostics(job['id'])
        assert not window.experiments.result(job['id'])['preflight']
    finally:
        doc.dirty = False; window.close(); application.processEvents()
        window.deleteLater(); QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)


def test_preflight_connection_errors_identify_ports_units_and_source_lines(builder, tmp_path):
    manager = Experiments(root=tmp_path)
    try:
        job = manager.create({'preflight': True, 'system': '''
let heat = part("housing", "thermal.capacitance", #{heat_capacity:20.0});
let command = part("command", "control.constant", #{value:1.0});
connect([heat.port("node"), command.port("value")]);
''', 'controller': None})
        record, _ = wait(manager, job)
        assert record['state'] == 'failed'
        error = record['error']
        for text in ('housing.node', 'command.value', 'K / W', 'output [1]', 'system.rhai:4:'):
            assert text in error, error
        assert manager.components(job['id'])['imported'] == []
        checked = manager.create({'preflight': True, 'system': '''
let disk = part("disk", "rotational.inertia", #{inertia:1.0, "initial.speed":2.0});
connect([disk.port("shaft")]);''', 'controller': None})
        record, _ = wait(manager, checked)
        assert record['state'] == 'completed', manager.diagnostics(checked['id'])
        result = manager.result(checked['id'])
        assert result['trace'] == {} and result['duration_s'] == 0
        assert [c['name'] for c in manager.components(checked['id'])['resolved']] == ['disk']
    finally:
        manager.close()


def test_initial_conditions_reach_provided_states_and_conflicts_keep_source_locations(builder, tmp_path):
    manager = Experiments(root=tmp_path)
    script = '''let rotor = part("rotor", "rotational.inertia", #{inertia:1.0, "initial.shaft.speed":4.0});
connect([rotor.port("shaft")]);'''
    req = {'system': script, 'controller': None, 'settings': {'seconds': .1, 'step': .001, 'sample': .01}}
    try:
        job = manager.create(req); record, _ = wait(manager, job)
        assert record['state'] == 'completed', manager.diagnostics(job['id'])
        series = signals(manager.result(job['id']))
        assert series['rotor.shaft.angle']['values'][-1] == pytest.approx(.4, abs=1e-10)
        assert series['rotor.speed']['values'][-1] == pytest.approx(4.)
        bad = script.replace('inertia:1.0,', 'inertia:1.0, "initial.speed":3.0,')
        job = manager.create({**req, 'system': bad, 'preflight': True}); record, _ = wait(manager, job)
        assert record['state'] == 'failed'
        for expected in ['conflicting initial values', 'rotor.initial.speed', 'rotor.initial.shaft.speed', 'rad/s', 'system.rhai:1:', 'system.rhai:2:']:
            assert expected in record['error'], record['error']
        orphan = 'let reading = part("reading", "sensor.imu", #{period:0.01});\nconnect([reading.port("frame")]);'
        job = manager.create({**req, 'system': orphan, 'preflight': True}); record, _ = wait(manager, job)
        assert record['state'] == 'failed'
        for expected in ['reading.frame', 'exactly one frame owner', 'found 0', 'system.rhai:1:']:
            assert expected in record['error'], record['error']
        orientation = '''let body = part("body", "multibody.rigid_body", #{mass:1, ixx:1, iyy:1, izz:1, "initial.qx":0.6});
let sphere = part("sphere", "multibody.sphere_contact", #{radius:0.1, stiffness:1000, "initial.frame.qw":0.8, "initial.frame.z":1});
connect([sphere.port("frame"), body.port("frame")]);'''
        job = manager.create({**req, 'system': orientation}); record, _ = wait(manager, job)
        assert record['state'] == 'completed', manager.diagnostics(job['id'])
        values = signals(manager.result(job['id']))
        assert values['body.qw']['values'][-1] == pytest.approx(.8)
        invalid = orientation.replace('"initial.frame.qw":0.8', '"initial.frame.qw":1.0')
        job = manager.create({**req, 'system': invalid, 'preflight': True}); record, _ = wait(manager, job)
        assert record['state'] == 'failed'
        for expected in ['initial quaternion', 'unit length', 'body.frame', 'sphere.frame', 'system.rhai:1:', 'system.rhai:2:']:
            assert expected in record['error'], record['error']
    finally:
        manager.close()


def test_coupled_motor_case_derivation_sensor_and_geometry_iteration(builder, tmp_path):
    from robocad.api import ApiServer
    from robocad.client import RoboClient
    # The fixture loads the portable builder without installing it as a package.
    spec = importlib.util.spec_from_file_location('thermal_build', HERE/'build_model.py')
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    doc, ids = module.build_thermal(); manager = Experiments(doc, root=tmp_path)
    server = ApiServer(doc, port=0); server.service._experiments = manager; server.start()
    client = RoboClient(server.url)
    req = request(doc); req['system'] = 'let assembly = cad("assembly");'
    req['settings'] = {'seconds': 1., 'step': .0005, 'sample': .005}
    try:
        job = manager.create(req); record, _ = wait(manager, job)
        assert record['state'] == 'completed', manager.diagnostics(job['id'])
        result = manager.result(job['id']); channels = signals(result)
        temperature = channels['graph/housing.node.temperature']
        assert temperature['node_ids'] == [ids['motor']]
        assert temperature['component_id'] == 'housing'
        captured_sources = client.experiment_sources(job['id'])
        declaration = temperature['source']
        assert 'graph/housing' in captured_sources['system']['files'][declaration['path']].splitlines()[declaration['line']-1]
        assert captured_sources['controller']['files']['controller.rhai'] == req['controller']['sources']['files']['controller.rhai']
        np.testing.assert_array_equal(temperature['values'], channels['graph/sensor.temperature']['values'])
        assert sum(c['kind'] == 'robot.motor_unit' for c in result['components']) == 1
        assert not any(c['name'].startswith('graph/') and c['kind'] == 'thermal.capacitance' for c in result['components'])
        tc = np.asarray(temperature['values']); t = np.asarray(temperature['t'])
        tw = np.asarray(channels['graph/winding.node.temperature']['values'])
        tm = np.asarray(channels['graph/mount.node.temperature']['values'])
        ta = np.asarray(channels['graph/ambient.node.temperature']['values'])
        capacity = result['component_derivations'][0]['outputs']['heat_capacity']['value']
        assert tc[-1] > tc[0] and tw[-1] > tc[-1]
        # Independent case heat balance, integrated from recorded temperatures.
        power = (tw-tc) - .2*(tc-ta) - .1*(tc-tm)
        energy = np.sum(.5*(power[1:]+power[:-1])*np.diff(t))
        assert capacity*(tc[-1]-tc[0]) == pytest.approx(energy, rel=.02, abs=1e-7)
        assert np.ptp(result['trace']['joints']['hinge1']) > .05
        assert len(result['controller_frames']) > 10
        start = time.perf_counter()
        repeat = client.post('/experiments', req)
        acknowledgement = time.perf_counter()-start
        record, _ = wait(manager, repeat)
        elapsed = time.perf_counter()-start
        assert record['state'] == 'completed', manager.diagnostics(repeat['id'])
        repeated = manager.result(repeat['id'])
        assert repeated['cache']['cad_hit'] is True
        assert repeated['cache']['component_derivations']['component_recipes']['hits'] == 1
        assert repeated['trace'] == result['trace']
        assert repeated['controller_frames'] == result['controller_frames']
        assert acknowledgement < float(os.environ.get('EXPERIMENT_ACK_SECONDS', '.1'))
        assert elapsed < float(os.environ.get('EXPERIMENT_THERMAL_SECONDS', '6'))
        client.update_system_component('cooling', {'parameters': {'conductance': .4}}, doc.revision)
        req['expected_revision'] = doc.revision
        cooled = client.post('/experiments', req); record, _ = wait(manager, cooled)
        assert record['state'] == 'completed', manager.diagnostics(cooled['id'])
        cooling_result = manager.result(cooled['id'])
        assert cooling_result['cache']['cad_hit'] is True
        assert signals(cooling_result)['graph/housing.node.temperature']['values'][-1] < tc[-1]
        client.update_system_component('cooling', {'parameters': {'conductance': .2}}, doc.revision)
        # A body edit changes the derived storage without rewriting its graph.
        Ops(doc).transform([ids['motor']], scale=1.2)
        Ops(doc).rename(ids['motor'], 'Renamed drive')
        req['expected_revision'] = doc.revision
        changed = manager.create(req); record, _ = wait(manager, changed)
        assert record['state'] == 'completed', manager.diagnostics(changed['id'])
        candidate = manager.result(changed['id'])
        c2 = candidate['component_derivations'][0]['outputs']['heat_capacity']['value']
        assert c2 == pytest.approx(capacity*1.2**3, rel=1e-8)
        assert candidate['cache']['cad_hit'] is False
        assert manager.result(job['id'])['stale']
        assert signals(candidate)['graph/housing.node.temperature']['values'][-1] < tc[-1]
        assert client.experiment_sources(job['id']) == captured_sources
        (tmp_path/'thermal-acceptance.json').write_text(json.dumps({
            'case_capacity_j_per_k': capacity, 'scaled_capacity_j_per_k': c2,
            'case_stored_energy_j': capacity*(tc[-1]-tc[0]), 'integrated_case_heat_j': energy,
            'heat_balance_relative_error': abs(capacity*(tc[-1]-tc[0])-energy)/abs(energy),
            'cached_acknowledgement_s': acknowledgement, 'cached_total_s': elapsed,
            'trace_repeat_exact': True, 'controller_repeat_exact': True,
            'run_ids': [job['id'], repeat['id'], cooled['id'], changed['id']],
        }, indent=2))
    finally:
        server.stop()
        manager.close()


def test_coupled_inspector_run_compare_replay_and_evidence_ui(builder, tmp_path, monkeypatch):
    from functools import partial
    from PySide6.QtCore import QCoreApplication, QEvent, Qt
    from PySide6.QtTest import QTest
    from PySide6.QtWidgets import QApplication, QPushButton
    from robocad.document import Document
    import robocad.ui.app as ui
    spec = importlib.util.spec_from_file_location('thermal_ui_build', HERE/'build_model.py')
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    application = QApplication.instance() or QApplication([])
    monkeypatch.setattr(ui.MainWindow, 'start_api', lambda self: None)
    monkeypatch.setattr(Document, 'start_autosave', lambda *args: None)
    monkeypatch.setattr(ui, 'Experiments', partial(Experiments, root=tmp_path))
    doc, ids = module.build_thermal(); window = ui.MainWindow(doc)
    try:
        window.resize(1500, 1000); window.show(); window.experiments_dock.show()
        panel = window.experiments_panel
        panel.system.setPlainText('let assembly = cad("assembly");')
        panel.parameters.setPlainText(json.dumps({'system': {}, 'controller': {'target': .2},
            'settings': {'seconds': .5, 'step': .0005, 'sample': .005}}))
        graph = panel.graph_panel; graph.receive_catalogue(window.experiments.catalogue())
        button = panel.findChild(QPushButton, 'primaryAction')
        QTest.mouseClick(button, Qt.LeftButton)
        first_id = panel.current_id()
        record, _ = wait(window.experiments, {'id': first_id}, application.processEvents)
        assert record['state'] == 'completed', window.experiments.diagnostics(first_id)
        QTest.qWait(20)
        assert panel.baseline.isEnabled(), panel.status.text()
        panel.baseline.click()
        assert panel.baseline_id == first_id
        panel.tabs.setCurrentWidget(graph); graph.choose_component('cooling')
        row = next(i for i in range(graph.parameters.rowCount()) if graph.parameters.item(i, 0).text() == 'conductance')
        assert graph.parameters.item(row, 2).text() == 'W/K'
        graph.parameters.item(row, 1).setText('0.4'); graph.apply_button.click()
        assert doc.component_graph['components']['cooling']['parameters']['conductance'] == .4
        QTest.mouseClick(button, Qt.LeftButton)
        second_id = panel.current_id(); assert first_id != second_id
        record, _ = wait(window.experiments, {'id': second_id}, application.processEvents)
        assert record['state'] == 'completed', window.experiments.diagnostics(second_id)
        QTest.qWait(20)
        assert window.experiments.result(second_id)['cache']['cad_hit'] is True
        review = panel.review()
        assert review.baseline['run_id'] == first_id
        assert 'components and connections' in review.metrics.toPlainText()
        review.part.setCurrentIndex(review.part.findData(ids['motor']))
        review.signal.setCurrentIndex(review.signal.findData('graph/housing.node.temperature'))
        assert review.signal.currentData() == 'graph/housing.node.temperature'
        assert review.component_action.isEnabled() and review.source_action.isEnabled()
        source = review.view_source()
        assert source.editor.isReadOnly()
        assert 'graph/housing' in source.editor.textCursor().selectedText()
        source.close()
        review.inspect_component()
        assert panel.graph_panel.current == 'housing' and panel.tabs.currentWidget() is panel.graph_panel
        review.raise_(); review.activateWindow()
        review.seek(.25)
        assert review.times[review.slider.value()] == pytest.approx(.25)
        assert review.viewport is not None and review.doc is not doc
        review.note.setPlainText('Higher case-to-air conductance lowers the recorded housing temperature.')
        review.annotate()
        evidence = next(reversed(doc.annotations.values()))['evidence']
        assert evidence['run_id'] == second_id and evidence['node_ids'] == [ids['motor']]
        assert evidence['signal'] == 'graph/housing.node.temperature'
        assert evidence['source']['path'].endswith('__robocad_graph.rhai')
        application.processEvents()
        review.grab().save(str(tmp_path/'thermal-review.png'))
        (tmp_path/'thermal-ui-evidence.json').write_text(json.dumps({'baseline': first_id, 'candidate': second_id,
            'annotation': evidence, 'sample_s': review.times[review.slider.value()]}))
        # Live graph removal disables editing navigation but preserves captured evidence.
        window.ops.set_component_graph({'version': 1, 'components': {}, 'connections': {}})
        QTest.qWait(20)
        assert not review.component_action.isEnabled() and review.source_action.isEnabled()
        source = review.view_source()
        assert 'graph/housing' in source.editor.textCursor().selectedText()
        source.close()
        window.ops.undo(); QTest.qWait(20)
        assert review.component_action.isEnabled()
        review.close()
    finally:
        doc.dirty = False; window.close(); application.processEvents()
        window.deleteLater(); QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)


@pytest.mark.parametrize('derive_capacity', [False, True], ids=['manual-capacity', 'cad-capacity'])
def test_captured_component_graph_runs_native_thermal_components(builder, tmp_path, derive_capacity):
    from robocad.document import Document
    from robocad.component_graph import empty_graph
    doc = Document(); ops = Ops(doc)
    body = ops.box((0, 0, 0), (10, 10, 10))
    graph = empty_graph()
    graph['components'] = {
        'housing': {'id': 'housing', 'name': 'Housing', 'body_id': body,
            'type': 'thermal.capacitance', 'parameters': {'heat_capacity': 20., 'initial.temperature': 300.}},
        'winding': {'id': 'winding', 'name': 'Winding loss', 'body_id': body,
            'type': 'thermal.heat_source', 'parameters': {'power': 10.}},
    }
    graph['connections']['heat'] = {'id': 'heat', 'ports': [
        {'component_id': 'housing', 'port': 'node'}, {'component_id': 'winding', 'port': 'node'}]}
    capacity = 20.
    if derive_capacity:
        graph['components']['housing']['parameters'].pop('heat_capacity')
        graph['components']['housing']['derivation'] = {'kind': 'body_thermal_capacity', 'specific_heat': 1000.}
        capacity = 1.24  # 1 cm³ PLA, 1.24 g/cm³, explicit 1000 J/(kg·K).
    ops.set_component_graph(graph)
    manager = Experiments(doc, root=tmp_path)
    try:
        job = manager.create({'expected_revision': doc.revision,
            'system': {'entry': 'system.rhai', 'files': {'system.rhai': ''}},
            'controller': None, 'settings': {'seconds': 1., 'step': .01, 'sample': .01}})
        graph['components']['winding']['parameters']['power'] = 100.
        ops.set_component_graph(graph)
        record, _ = wait(manager, job)
        assert record['state'] == 'completed', manager.diagnostics(job['id'])
        result = manager.result(job['id'])
        assert result['stale']
        assert {m['body_id'] for m in result['component_graph_mapping']} == {body}
        channels = signals(result)
        temperatures = [s for s in channels.values() if s['unit'] == 'K']
        assert temperatures, channels
        # dT/dt = Q/C; this tests the captured 10 W graph even
        # though the document now asks for 100 W.
        for signal in temperatures:
            np.testing.assert_allclose(signal['values'], 300. + (10./capacity)*np.asarray(signal['t']), atol=1e-8)
        if derive_capacity:
            evidence = result['component_derivations'][0]
            assert evidence['outputs']['heat_capacity']['value'] == pytest.approx(capacity)
            assert evidence['inputs']['specific_heat_source'] == 'recipe'
            repeat = manager.create({'expected_revision': doc.revision,
                'system': '', 'controller': None, 'settings': {'seconds': 1., 'step': .01, 'sample': .01}})
            repeat_record, _ = wait(manager, repeat)
            assert repeat_record['state'] == 'completed', manager.diagnostics(repeat['id'])
            repeated = manager.result(repeat['id'])
            assert repeated['cache']['component_derivations']['component_recipes']['hits'] == 1
            for signal in signals(repeated).values():
                if signal['unit'] == 'K':
                    np.testing.assert_allclose(signal['values'], 300. + (100./capacity)*np.asarray(signal['t']), atol=1e-8)
    finally:
        manager.close()


@pytest.mark.parametrize('mechanical_attachment', [False, True], ids=['fluid-proxy', 'invalid-mechanical-proxy'])
def test_explicit_fluid_volume_excludes_solid_mass_and_rejects_mechanical_use(builder, tmp_path, mechanical_attachment):
    from robocad.component_graph import empty_graph
    doc, ids = builder(False); ops = Ops(doc)
    body = ops.cylinder((100., 0., 100.), (1., 0., 0.), 5., 100., name='Fluid volume')
    graph = empty_graph()
    graph['components']['duct'] = {'id': 'duct', 'name': 'Water passage', 'type': 'fluid.pipe_ph',
        'body_id': body, 'parameters': {}, 'derivation': {'kind': 'circular_fluid_volume'}}
    for identity, port in [('inlet', 'a'), ('outlet', 'b')]:
        graph['components'][identity] = {'id': identity, 'name': identity, 'type': 'fluid.reservoir_ph', 'parameters': {}}
        graph['connections'][identity] = {'id': identity, 'ports': [
            {'component_id': 'duct', 'port': port}, {'component_id': identity, 'port': 'node'}]}
    ops.set_component_graph(graph)
    if mechanical_attachment: ops.connect_fixed(ids['base'], body)
    manager = Experiments(doc, root=tmp_path)
    try:
        job = manager.create({'expected_revision': doc.revision, 'system': 'let assembly = cad("assembly");',
            'controller': None, 'settings': {'seconds': .02, 'step': .001, 'sample': .01}})
        record, _ = wait(manager, job)
        if mechanical_attachment:
            assert record['state'] == 'failed'
            assert 'mechanically references a fluid-volume proxy' in record['error']
            return
        assert record['state'] == 'completed', manager.diagnostics(job['id'])
        result = manager.result(job['id'])
        evidence = result['component_derivations'][0]
        assert {k: v['value'] for k, v in evidence['outputs'].items()} == pytest.approx({'length': .1, 'diameter': .01, 'rise': 0.})
        physical = json.loads((tmp_path/job['id']/'physical.json').read_text())
        assert all(body not in [link['id'], *link.get('members', [])] for link in physical['links'])
        assert body in doc.nodes  # Exclusion affects the captured mechanical export only.
        assert signals(result)['graph/duct.a.pressure']['node_ids'] == [body]
        assert len(result['trace']['poses']) > 0
    finally:
        manager.close()


@pytest.mark.parametrize('configuration,diagnostic', [
    ('#{settings:#{stepp:0.01}}', 'stepp'),
    ('#{settings:#{contact:"yes"}}', 'settings.contact must be a boolean'),
    ('#{cad_overrides:[#{section:"links",id:"missing",field:"/mass",value:1.0}]}',
     'cad_overrides requires an imported CAD assembly'),
])
def test_configuration_failures_identify_the_captured_module(builder, tmp_path, configuration, diagnostic):
    manager = Experiments(root=tmp_path)
    try:
        job = manager.create({'system': {'entry': 'system.rhai', 'files': {
            'system.rhai': 'import "scenario" as scenario;',
            'scenario.rhai': '\n\nconfigure('+configuration+');',
        }}})
        record, _ = wait(manager, job)
        assert record['state'] == 'failed', record
        assert 'scenario.rhai:3:' in record['error'], record
        assert diagnostic in record['error'], record
    finally:
        manager.close()


@pytest.mark.parametrize('two_joint', [False, True], ids=['pendulum', 'two-joint'])
def test_cad_rhai_measured_replay_and_validated_cache(builder, tmp_path, two_joint):
    doc, ids = builder(two_joint)
    manager = Experiments(doc, root=tmp_path)
    before = capture(doc); req = request(doc)
    start = time.monotonic(); job = manager.create(req); ack = time.monotonic()-start
    # Editing the caller's source map after acknowledgement cannot change a run.
    req['controller']['sources']['files']['reference.rhai'] = 'fn step(t, target) { 99.0 }'
    record, _ = wait(manager, job)
    assert record['state'] == 'completed', manager.diagnostics(job['id'])
    cold = manager.result(job['id'])
    assert cold['evaluation']['status'] == 'passed'
    assert not cold['cache']['cad_hit'] and not cold['stale']
    assert capture(doc).data == before.data
    assert len(cold['trace']['t']) == 320
    assert len(cold['controller_frames']) == 160
    assert all(f['commands']['hinge1.target'] <= .2 for f in cold['controller_frames'])
    assert cold['controller_contract']['period'] == .02
    assert {c['unit'] for c in cold['controller_contract']['actuators']} == {'rad'}
    # Independent homogeneous transforms about the two original CAD pivots.
    def rotation(q, z):
        c, s = np.cos(q), np.sin(q)
        r = np.array([[c, 0, s], [0, 1, 0], [-s, 0, c]])
        m = np.eye(4); m[:3, :3] = r; p = np.array([0., 0., z]); m[:3, 3] = p-r@p
        return m
    for i in (0, 25, 90, 319):
        matrices = replay_matrices(cold, i)
        first = rotation(cold['trace']['joints']['hinge1'][i], 200)
        np.testing.assert_allclose(matrices[ids['first']], first, atol=1e-7)
        last = first
        if two_joint:
            last = first @ rotation(cold['trace']['joints']['hinge2'][i], 80)
            np.testing.assert_allclose(matrices[ids['second']], last, atol=1e-7)
        np.testing.assert_allclose(matrices[ids['attachment']], last, atol=1e-7)
    assert ids['first'] in signals(cold)['joints/hinge1/angle']['node_ids']
    start = time.monotonic(); cached_job = manager.create(request(doc))
    cached_record, _ = wait(manager, cached_job)
    elapsed = time.monotonic()-start
    assert cached_record['state'] == 'completed', manager.diagnostics(cached_job['id'])
    cached = manager.result(cached_job['id'])
    assert cached['cache']['cad_hit']
    assert cold['trace'] == cached['trace'] and cold['controller_frames'] == cached['controller_frames']
    report = {'ack_ms': ack*1000, 'cached_wall_s': elapsed, 'cold_timing': cold['timing'], 'cached_timing': cached['timing']}
    (tmp_path/'performance.json').write_text(json.dumps(report, indent=2))
    # Preserve host measurements; the dedicated CI profile sets its wall budget.
    assert ack < float(os.environ.get('EXPERIMENT_ACK_SECONDS', '.1')), report
    budget = os.environ.get('EXPERIMENT_TWO_JOINT_SECONDS' if two_joint else 'EXPERIMENT_CACHED_SECONDS')
    if budget: assert elapsed < float(budget), report
    # Corruption must trigger regeneration rather than corrupting subsequent runs.
    if not two_joint:
        cache_model = next((tmp_path/'cache').glob('*/model.json'))
        cache_model.write_text('{"tampered": true}')
        repaired_job = manager.create(request(doc)); repaired_record, _ = wait(manager, repaired_job)
        assert repaired_record['state'] == 'completed', manager.diagnostics(repaired_job['id'])
        repaired = manager.result(repaired_job['id'])
        assert not repaired['cache']['cad_hit'] and repaired['trace'] == cold['trace']
    Ops(doc).transform([ids['first']], scale=1.02)
    assert manager.result(job['id'])['stale']
    assert not json.loads((tmp_path/job['id']/'result.json').read_text()).get('stale', False)


def test_background_failure_cancellation_and_qt_heartbeat(builder, tmp_path):
    os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
    from PySide6.QtCore import QTimer
    from PySide6.QtWidgets import QApplication
    app = QApplication.instance() or QApplication([])
    beats = []; timer = QTimer(); timer.setInterval(10); timer.timeout.connect(lambda: beats.append(time.monotonic())); timer.start()
    doc, _ = builder(False); manager = Experiments(doc, root=tmp_path)
    job = manager.create(request(doc)); record, _ = wait(manager, job, app.processEvents)
    assert record['state'] == 'completed', manager.diagnostics(job['id'])
    assert len(beats) > 20
    assert max(b-a for a, b in zip(beats, beats[1:])) < .1
    bad = request(doc); bad['controller']['sources']['files']['controller.rhai'] = 'fn control(t, s, a, state) { throw "controller failed on purpose"; }'
    failure = manager.create(bad); failed, _ = wait(manager, failure, app.processEvents)
    assert failed['state'] == 'failed' and 'controller failed on purpose' in failed['error']
    assert manager.get(job['id'])['state'] == 'completed'
    invalid = request(doc); invalid['system']['files']['system.rhai'] = 'let impossible = ;'
    failure = manager.create(invalid); failed, _ = wait(manager, failure, app.processEvents)
    assert failed['state'] == 'failed' and 'system.rhai' in failed['error']
    long = request(doc); long['settings'] = {'seconds': 100.}
    running = manager.create(long)
    start = time.monotonic()
    while manager.get(running['id'])['state'] != 'running':
        app.processEvents(); time.sleep(.005)
        assert time.monotonic()-start < 20
    pid = manager.get(running['id'])['pid']
    queued = manager.create(request(doc)); manager.cancel(queued['id'])
    assert manager.get(queued['id'])['state'] == 'cancelled'
    start = time.monotonic(); manager.cancel(running['id']); ack = time.monotonic()-start
    cancelled, _ = wait(manager, running, app.processEvents, timeout=2)
    assert ack < .25 and cancelled['state'] == 'cancelled'
    if os.name != 'nt':
        # The group includes the Python worker, Rust runner and any controllers.
        while time.monotonic()-start < 2:
            try: os.killpg(pid, 0)
            except ProcessLookupError: break
            time.sleep(.01)
        else: pytest.fail('Worker process group survived cancellation')
    timer.stop(); manager.close()


def test_script_only_runs_and_parameters_use_same_worker_without_cad(builder, tmp_path):
    from robocad.document import Document
    doc = Document(); manager = Experiments(doc, root=tmp_path)
    script = '''let p = parameters();
        let disk = part("disk", "rotational.inertia", #{inertia: p.inertia, damping: 0.5, "initial.speed": 3.0});
        connect([disk.port("shaft")]);'''
    request = {'expected_revision': doc.revision, 'system': script, 'parameters': {'inertia': 2.},
               'controller': None, 'settings': {'seconds': .2, 'step': .001, 'sample': .01}}
    job = manager.create(request); record, _ = wait(manager, job)
    assert record['state'] == 'completed', manager.diagnostics(job['id'])
    a = manager.result(job['id'])
    assert manager.captured_document(job['id']) is None
    series = signals(a)
    speed = next(v for k, v in series.items() if k.endswith('speed'))
    assert speed['values'][-1] == pytest.approx(3*np.exp(-.5/2*.2), abs=3e-5)
    request['parameters']['inertia'] = 1.
    second = manager.create(request); record, _ = wait(manager, second)
    assert record['state'] == 'completed', manager.diagnostics(second['id'])
    b = manager.result(second['id'])
    faster = next(v for k, v in signals(b).items() if k.endswith('speed'))
    assert faster['values'][-1] < speed['values'][-1]
    Ops(doc).box((0,0,0),(10,10,10))
    assert not manager.result(job['id'])['stale']


def test_sensor_graph_api_rhai_run_retains_units_sources_and_seeded_channels(builder, tmp_path):
    from robocad.api import ApiServer
    from robocad.client import RoboClient
    from robocad.document import Document
    doc = Document(); manager = Experiments(doc, root=tmp_path)
    server = ApiServer(doc, port=0); server.service._experiments = manager; server.start()
    client = RoboClient(server.url)
    try:
        catalog = client.get('/experiments/catalogue')
        imu = next(c for c in catalog if c['type'] == 'sensor.imu')
        assert imu['parameters_complete']
        assert next(p for p in imu['ports'] if p['name'] == 'ax')['unit'] == 'm/s²'
        component = {'name': 'Inertial reading', 'type': 'sensor.imu',
            'parameters': {'period': .001, 'noise.ax': .2, 'noise.ay': .2, 'seed': 17.,
                'initial.frame.x': .75, 'initial.vx': 2.}}
        identity = client.add_system_component(component, doc.revision)['id']
        native_name = 'graph/'+identity
        req = {'system': '''let body = part("body", "planar.rigid_body", #{mass:1.0, inertia:0.1, gravity:0.0});
            connect([component("GRAPH_READING").port("frame"), body.port("frame")]);'''.replace('GRAPH_READING', native_name),
            'controller': None, 'settings': {'seconds': 1., 'step': .00025, 'sample': .001}}
        recorded = []
        for seed in [17., 17., 18.]:
            client.update_system_component(identity, {'parameters': {**component['parameters'], 'seed': seed}}, doc.revision)
            job = client.post('/experiments', {**req, 'expected_revision': doc.revision})
            record, _ = wait(manager, job)
            assert record['state'] == 'completed', manager.diagnostics(job['id'])
            result = manager.result(job['id']); series = signals(result)
            ax, ay, gyro = (series[native_name+'.'+name] for name in ['ax', 'ay', 'gyro'])
            assert ax['unit'] == ay['unit'] == 'm/s²' and gyro['unit'] == 'rad/s'
            assert ax['component_id'] == identity and ax['source']['path'].endswith('__robocad_graph.rhai')
            assert np.max(np.abs(gyro['values'])) < 1e-12
            assert series['body.x']['values'][-1] == pytest.approx(.75 + 2., abs=1e-10)
            assert np.max(np.abs(np.asarray(ax['values']) - (np.asarray(ay['values']) - 9.81))) > .01
            acceleration_noise = [np.asarray(ax['values']), np.asarray(ay['values'])-9.81]
            assert all(len(v) == 1000 for v in acceleration_noise)
            for values in acceleration_noise:
                assert abs(values.mean()) < .03  # < 4.8 standard errors
                assert .18 < values.std(ddof=1) < .22  # declared sigma 0.2 m/s², ±10%
            assert abs(np.corrcoef(acceleration_noise)[0, 1]) < .1
            recorded.append(result['trace'])
        assert recorded[0] == recorded[1] and recorded[0] != recorded[2]
    finally:
        server.stop(); manager.close()


@pytest.mark.skipif(os.name == 'nt', reason='Worker lease inheritance uses POSIX descriptors')
def test_worker_finishes_after_editor_process_exits(builder, tmp_path):
    import subprocess
    code = '''import os,sys,time
from robocad.experiments import Experiments
m=Experiments(root=sys.argv[1])
j=m.create({'system':'let d=part("disk","rotational.inertia",#{inertia:1.0});connect([d.port("shaft")]);',
            'settings':{'seconds':1.0}})
print(j['id'],flush=True)
while 'pid' not in m.get(j['id']): time.sleep(.001)
os._exit(0)
'''
    env = dict(os.environ, PYTHONPATH=str(ROOT/'cad'))
    editor = subprocess.run([sys.executable, '-c', code, str(tmp_path)], capture_output=True, text=True, env=env, timeout=20)
    assert editor.returncode == 0, editor.stderr
    run_id = editor.stdout.strip()
    observer = Experiments(root=tmp_path)
    record, _ = wait(observer, {'id': run_id})
    assert record['state'] == 'completed', record
    assert len(observer.result(run_id)['trace']['t']) == 100
    observer.close()


def test_python_reference_parity_uses_captured_sources_and_dependencies(builder, tmp_path):
    doc, _ = builder(False); manager = Experiments(doc, root=tmp_path/'runs')
    req = request(doc); req['settings'] = {'seconds': .8}
    job = manager.create(req); record, _ = wait(manager, job)
    assert record['state'] == 'completed', manager.diagnostics(job['id'])
    rhai = manager.result(job['id'])
    helper = tmp_path/'reference.py'; helper.write_text((HERE/'reference.py').read_text())
    req['controller'] = {'language': 'process', 'parameters': {'target1': .2, 'target2': -.15},
        'process': {'runtime': 'python', 'entry': 'controller.py', 'files': {
            'controller.py': {'path': str(HERE/'controller.py')},
            'reference.py': {'path': str(helper)},
            **{f'simloop/{p.name}': {'path': str(p)} for p in (ROOT/'clients/python/simloop').glob('*.py')},
        }}}
    job = manager.create(req)
    helper.write_text('def step(t,target): return 99.0\n')
    record, _ = wait(manager, job)
    assert record['state'] == 'completed', manager.diagnostics(job['id'])
    reference = manager.result(job['id'])
    assert reference['cache']['cad_hit']
    assert reference['controller_contract'] == rhai['controller_contract']
    assert reference['controller_frames'] == rhai['controller_frames']
    assert reference['trace'] == rhai['trace']
    # A missing import must fail instead of borrowing a dependency from the
    # editor's live PYTHONPATH. The previous successful run stays available.
    req['controller']['process']['files'] = {k: v for k, v in req['controller']['process']['files'].items() if not k.startswith('simloop/')}
    failed = manager.create(req); record, _ = wait(manager, failed)
    assert record['state'] == 'failed'
    assert 'No module named' in manager.diagnostics(failed['id'])['stderr']
    assert manager.get(job['id'])['state'] == 'completed'
    manager.close()


@pytest.mark.skipif(os.name == 'nt', reason='POSIX process-group cancellation')
def test_cancellation_kills_a_process_controller_ignoring_sigterm(builder, tmp_path):
    doc, _ = builder(False); manager = Experiments(doc, root=tmp_path)
    req = request(doc)
    code = '''import json,os,signal,sys,time
signal.signal(signal.SIGTERM, signal.SIG_IGN)
sys.stdin.readline(); print('{"type":"ready"}',flush=True)
with open('started.tmp','w') as marker: json.dump({'pid':os.getpid()}, marker)
os.replace('started.tmp', 'started.json')
while True: time.sleep(.01)
'''
    req['controller'] = {'language': 'process', 'process': {'runtime': 'python', 'entry': 'controller.py', 'files': {'controller.py': code}}}
    try:
        job = manager.create(req); marker = tmp_path/job['id']/'controller'/'started.json'
        start = time.monotonic()
        while not marker.exists():
            assert manager.get(job['id'])['state'] not in TERMINAL, manager.diagnostics(job['id'])
            assert time.monotonic()-start < 30
            time.sleep(.01)
        pid = json.loads(marker.read_text())['pid']
        start = time.monotonic(); manager.cancel(job['id'])
        assert time.monotonic()-start < .25
        record, _ = wait(manager, job, timeout=2)
        assert record['state'] == 'cancelled'
        while time.monotonic()-start < 2:
            try: os.kill(pid, 0)
            except ProcessLookupError: break
            time.sleep(.01)
        else: pytest.fail('Controller ignoring SIGTERM survived process-group cancellation')
    finally:
        manager.close()


def test_independent_cad_system_and_controller_edits_change_captured_results(builder, tmp_path):
    doc, ids = builder(False); manager = Experiments(doc, root=tmp_path)
    def run(req):
        req['settings'] = {'seconds': .8}
        job = manager.create(req); record, _ = wait(manager, job)
        assert record['state'] == 'completed', manager.diagnostics(job['id'])
        return manager.result(job['id']), json.loads((tmp_path/job['id']/'physical.json').read_text())
    baseline, physical = run(request(doc))
    link = next(l for l in physical['links'] if ids['first'] in l['members'])
    control_request = request(doc); control_request['controller']['parameters']['target1'] = .1
    control, _ = run(control_request)
    assert control['cache']['cad_hit'] and control['controller_frames'][-1]['commands']['hinge1.target'] == .1
    assert control['trace']['joints']['hinge1'][-1] < baseline['trace']['joints']['hinge1'][-1]
    system_request = request(doc); system_request['parameters']['mass'] = link['mass']*2
    system_request['system'] = '''let assembly=cad("assembly");
        configure(#{cad_overrides:[#{section:"links",id:"%s",field:"/mass",value:parameters().mass}]});''' % link['id']
    system, overridden = run(system_request)
    assert system['cache']['cad_hit']
    assert next(l for l in overridden['links'] if l['id'] == link['id'])['mass'] == link['mass']*2
    assert system['configuration']['cad_overrides'][0]['before'] == link['mass']
    assert system['configuration']['cad_overrides'][0]['source']['source'] == 'system.rhai'
    assert system['trace'] != baseline['trace']
    Ops(doc).transform([ids['first']], scale=1.1)
    cad, resized = run(request(doc))
    assert not cad['cache']['cad_hit']
    assert cad['cache']['derived']['body_properties']['hits'] > 0
    assert cad['cache']['derived']['body_properties']['misses'] == 1
    assert next(l for l in resized['links'] if l['id'] == link['id'])['mass'] > link['mass']
    assert cad['trace'] != baseline['trace']
    assert len({r['provenance']['binary_hash'] for r in (baseline, control, system, cad)}) == 1
    manager.close()


def test_run_seed_controls_native_solver_noise_and_fresh_repeats(builder, tmp_path):
    manager = Experiments(root=tmp_path)
    script = '''let mass=part("mass","translational.mass",#{mass:1.0});
        let bath=part("bath","translational.langevin",#{damping:0.1,intensity:0.01});
        connect([mass.port("axis"),bath.port("axis")]);'''
    results = []
    for seed in (17, 18, 17):
        job = manager.create({'system': script, 'seed': seed, 'settings': {'seconds': .1, 'step': .001, 'sample': .01}})
        record, _ = wait(manager, job)
        assert record['state'] == 'completed', manager.diagnostics(job['id'])
        results.append(manager.result(job['id']))
    assert results[0]['trace'] == results[2]['trace']
    assert results[0]['trace'] != results[1]['trace']
    assert results[0]['seed'] == 17 and results[0]['solver']['integrator'] == 'backward_euler'
    manager.close()


def test_driver_interface_bypasses_firmware_and_exposes_measured_motor_channels(builder, tmp_path):
    doc, ids = builder(False); manager = Experiments(doc, root=tmp_path)
    req = {'expected_revision': doc.revision, 'system': 'let assembly = cad("assembly");',
           'settings': {'seconds': .4}, 'controller': {'language': 'rhai', 'interface': 'driver_duty',
           'parameters': {'duty': .05}, 'sources': '''fn control(t,s,a,state) {
               a["drive1.duty"] = if t < 0.2 { 0.0 } else { parameters().duty };
               #{commands:a,state:state}
           }'''}}
    job = manager.create(req); record, _ = wait(manager, job)
    assert record['state'] == 'completed', manager.diagnostics(job['id'])
    r = manager.result(job['id'])
    assert r['controller_interface'] == 'driver_duty'
    assert r['controller_contract']['actuators'] == [{'name': 'drive1.duty', 'unit': '1'}]
    sensors = {c['name']: c['unit'] for c in r['controller_contract']['sensors']}
    assert sensors['drive1.current'] == 'A' and sensors['drive1.torque'] == 'N·m'
    assert sensors['drive1.speed'] == 'rad/s' and sensors['hinge1.angle'] == 'rad'
    assert not any(c['name'].endswith('.firmware') for c in r['components'])
    assert sum(c['kind'] == 'robot.motor_unit' for c in r['components']) == 1
    assert max(abs(v) for v in r['trace']['motors']['drive1']['current']) > .01
    req['controller']['parameters']['duty'] = 1.1
    failed = manager.create(req); record, _ = wait(manager, failed)
    assert record['state'] == 'failed' and 'within [-1, 1]' in record['error']
    partial = manager.partial(failed['id'])
    assert partial['partial'] and len(partial['trace']['t']) > 5 and 'evaluation' not in partial
