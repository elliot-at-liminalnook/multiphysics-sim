"""Exercise the real Qt review controls against immutable captured artifacts."""
import json
from functools import partial
from pathlib import Path

import numpy as np
import pytest
from PySide6.QtWidgets import QApplication
from PySide6.QtCore import QCoreApplication, QEvent

from robocad.document import Document
from robocad.commands import Ops
from robocad.experiments import Experiments, write_json
from robocad.snapshots import capture
import robocad.ui.app as ui


def test_component_inspector_edits_connects_and_rejects_stale_forms(window):
    from PySide6.QtCore import Qt
    from PySide6.QtTest import QTest
    app, w, body, _ = window
    panel = w.experiments_panel.graph_panel
    if not Path(w.experiments.binary).exists(): pytest.skip('Build the native runner for registry-driven UI acceptance')
    panel.receive_catalogue(w.experiments.catalogue())
    w.viewport.selection.set_nodes([body])

    def parameter(name, value):
        row = next(i for i in range(panel.parameters.rowCount()) if panel.parameters.item(i, 0).text() == name)
        panel.parameters.item(row, 1).setText(str(value))
        return panel.parameters.item(row, 2).text()

    panel.types.setCurrentIndex(panel.types.findData('thermal.capacitance'))
    panel.add_button.click(); panel.name.setText('Housing'); panel.attach_selected()
    assert parameter('heat_capacity', 20) == 'J/K'
    assert parameter('initial.temperature', 300) == 'K'
    panel.apply_button.click(); housing = panel.current
    assert housing in w.doc.component_graph['components'], panel.status.text()
    assert w.doc.component_graph['components'][housing]['body_id'] == body
    panel.types.setCurrentIndex(panel.types.findData('thermal.heat_source'))
    panel.add_button.click(); panel.name.setText('Winding losses'); parameter('power', 10)
    panel.apply_button.click(); heater = panel.current
    assert heater != housing

    def click_port(identity):
        item = next(item for item in panel.view.scene().items() if item.data(0) ==
            ('port', {'component_id': identity, 'port': 'node'}))
        point = panel.view.mapFromScene(item.sceneBoundingRect().center())
        QTest.mouseClick(panel.view.viewport(), Qt.LeftButton, pos=point)
    click_port(housing)
    assert panel.pending_port == {'component_id': housing, 'port': 'node'}
    click_port(heater)
    assert panel.pending_port is None, panel.status.text()
    assert len(w.doc.component_graph['connections']) == 1
    connection = next(iter(w.doc.component_graph['connections']))
    panel.choose_connection(connection); panel.disconnect_button.click()
    assert not w.doc.component_graph['connections']
    w.ops.undo(); app.processEvents()
    assert len(w.doc.component_graph['connections']) == 1

    panel.choose_component(housing)
    parameter('heat_capacity', 30)
    w.ops.rename(body, 'Edited elsewhere')
    panel.apply_button.click()
    assert 'revision' in panel.status.text().lower()
    assert w.doc.component_graph['components'][housing]['parameters']['heat_capacity'] == 20
    # Reload explicitly, then apply to the new revision.
    panel.choose_component(housing); parameter('heat_capacity', 30); panel.apply_button.click()
    assert w.doc.component_graph['components'][housing]['parameters']['heat_capacity'] == 30


def test_component_inspector_saves_explicit_derivation_and_native_binding(window):
    from PySide6.QtCore import Qt
    app, w, body, _ = window
    panel = w.experiments_panel.graph_panel
    if not Path(w.experiments.binary).exists(): pytest.skip('Build the native runner for registry-driven UI acceptance')
    panel.receive_catalogue(w.experiments.catalogue())
    panel.types.setCurrentIndex(panel.types.findData('thermal.capacitance'))
    panel.add_button.click(); panel.name.setText('Derived motor case')
    panel.body.setCurrentIndex(panel.body.findData(body))
    panel.binding.setText(f'cad/{body}/case')
    panel.derivation.setCurrentIndex(panel.derivation.findData('body_thermal_capacity'))
    panel.specific_heat.setText('1000')
    row = next(i for i in range(panel.parameters.rowCount()) if panel.parameters.item(i, 0).text() == 'heat_capacity')
    assert not panel.parameters.item(row, 1).flags() & Qt.ItemIsEditable
    panel.apply_button.click()
    component = w.doc.component_graph['components'][panel.current]
    assert component['binding'] == f'cad/{body}/case'
    assert component['derivation'] == {'kind': 'body_thermal_capacity', 'specific_heat': 1000.}
    assert 'heat_capacity' not in component['parameters']
    w.ops.undo(); app.processEvents()
    assert not w.doc.component_graph['components']
    w.ops.redo(); app.processEvents(); panel.choose_component(component['id'])
    assert panel.derivation.currentData() == 'body_thermal_capacity'
    assert panel.specific_heat.text() == '1000.0'
    assert panel.binding.text() == f'cad/{body}/case'
    panel.view.zoom(.1)
    assert panel.view.transform().m11() == pytest.approx(.15)
    panel.view.focus_component(component['id'])
    assert panel.view.transform().m11() == 1.


def test_sensor_inspector_uses_channel_units_and_rejects_invalid_noise(window):
    app, w, body, _ = window
    panel = w.experiments_panel.graph_panel
    if not Path(w.experiments.binary).exists(): pytest.skip('Build the native runner for registry-driven UI acceptance')
    panel.receive_catalogue(w.experiments.catalogue())
    panel.types.setCurrentIndex(panel.types.findData('sensor.imu'))
    panel.add_button.click(); panel.name.setText('Inertial reading')
    panel.body.setCurrentIndex(panel.body.findData(body))
    rows = {panel.parameters.item(i, 0).text(): i for i in range(panel.parameters.rowCount())}
    for name, unit in [('noise.ax', 'm/s²'), ('quantum.gyro', 'rad/s'), ('period', 's')]:
        assert panel.parameters.item(rows[name], 2).text() == unit
    assert 'noise' not in rows
    panel.parameters.item(rows['period'], 1).setText('0.01')
    panel.parameters.item(rows['noise.ax'], 1).setText('-0.1')
    panel.apply_button.click()
    assert not w.doc.component_graph['components']
    assert 'noise.ax' in panel.status.text()
    panel.parameters.item(rows['noise.ax'], 1).setText('0.1')
    panel.apply_button.click()
    component = w.doc.component_graph['components'][panel.current]
    assert component['body_id'] == body and component['parameters']['noise.ax'] == .1


def test_dynamic_terrain_parameters_show_declared_units_while_editing_and_after_reload(window, tmp_path):
    from PySide6.QtCore import Qt
    app, w, body, _ = window
    panel = w.experiments_panel.graph_panel
    if not Path(w.experiments.binary).exists(): pytest.skip('Build the native runner for registry-driven UI acceptance')
    panel.receive_catalogue(w.experiments.catalogue())
    panel.types.setCurrentIndex(panel.types.findData('contact.point_terrain_compliant'))
    panel.add_button.click()
    rows = {panel.parameters.item(i, 0).text(): i for i in range(panel.parameters.rowCount())}
    panel.parameters.item(rows['stiffness'], 1).setText('10000')
    panel.parameters.item(rows['patches'], 1).setText('1')
    panel.parameter_row('', '', '')
    row = panel.parameters.rowCount()-1
    panel.parameters.item(row, 0).setText('patch0.x0')
    assert panel.parameters.item(row, 2).text() == 'm'
    panel.parameters.item(row, 1).setText('0')
    panel.parameters.item(row, 0).setText('patch0.misspelled')
    assert panel.parameters.item(row, 2).text() == 'Unknown'
    panel.apply_button.click()
    assert not w.doc.component_graph['components'] and 'unknown parameter' in panel.status.text()
    panel.parameters.item(row, 0).setText('patch0.x0')
    panel.parameter_row('patch0.x1', '1', '')
    panel.apply_button.click()
    assert w.doc.component_graph['components'][panel.current]['parameters']['patch0.x1'] == 1.
    panel.choose_component(panel.current)
    row = next(i for i in range(panel.parameters.rowCount()) if panel.parameters.item(i, 0).text() == 'patch0.x0')
    assert panel.parameters.item(row, 2).text() == 'm'
    assert panel.parameters.item(row, 0).flags() & Qt.ItemIsEditable
    w.resize(1500, 1050); w.show(); w.experiments_dock.show()
    w.experiments_dock.raise_()
    w.experiments_panel.tabs.setCurrentWidget(panel)
    panel.parameters.scrollToBottom(); app.processEvents()
    w.grab().save(str(tmp_path/'terrain-parameters.png'))


def test_large_mixed_graph_keeps_cad_run_actions_and_scrolled_edits_usable(window, tmp_path):
    from PySide6.QtCore import Qt
    from PySide6.QtTest import QTest
    from PySide6.QtWidgets import QPushButton
    app, w, body, _ = window
    panel = w.experiments_panel.graph_panel
    if not Path(w.experiments.binary).exists(): pytest.skip('Build the native runner for registry-driven UI acceptance')
    panel.receive_catalogue(w.experiments.catalogue())
    graph = {'version': 1, 'components': {}, 'connections': {}}
    families = [('thermal.capacitance', {'heat_capacity': 20}), ('thermal.conductance', {'conductance': .5}),
        ('thermal.heat_source', {'power': 10}), ('electrical.resistor', {'resistance': 2}),
        ('rotational.inertia', {'inertia': .1}), ('sensor.imu', {}),
        ('control.constant', {'value': 1}), ('fluid.volume_ph', {'volume': .01}), ('actuator.pwm_driver', {'supply': 12})]
    for i in range(18):
        kind, parameters = families[i % len(families)]
        identity = f'component{i}'
        graph['components'][identity] = {'id': identity, 'name': f'{i+1:02} · {kind} · housing temperature and controller feedback',
            'type': kind, 'body_id': body, 'parameters': parameters}
    for i in [0, 9]:
        graph['connections'][f'heat{i}'] = {'id': f'heat{i}', 'ports': [
            {'component_id': f'component{i}', 'port': 'node'}, {'component_id': f'component{i+1}', 'port': 'a'},
            {'component_id': f'component{i+2}', 'port': 'node'}]}
    w.ops.set_component_graph(graph)
    panel.choose_component('component5')
    w.resize(1280, 800); w.show(); w.experiments_dock.show(); w.experiments_dock.raise_()
    w.experiments_panel.tabs.setCurrentWidget(panel); app.processEvents()
    panel.view.focus_component('component5'); app.processEvents()
    assert w.height() <= 810 and w.width() <= 1300
    assert w.viewport.width() >= 350
    run = w.experiments_panel.findChild(QPushButton, 'primaryAction')
    assert run.isVisibleTo(w) and w.rect().contains(run.mapTo(w, run.rect().center()))
    bounds = panel.view.mapFromScene(panel.view.cards['component5'].sceneBoundingRect()).boundingRect()
    assert panel.view.viewport().rect().adjusted(-2, -2, 2, 2).contains(bounds)
    titles = [item for item in panel.view.scene().items() if item.data(0) == ('component', 'component5') and hasattr(item, 'toPlainText')]
    assert any(item.toolTip() == graph['components']['component5']['name'] and '…' in item.toPlainText() for item in titles)
    assert all(item.boundingRect().width() <= 258 for item in titles)
    # Reach a form action by scrolling its own inspector, without moving CAD.
    row = next(i for i in range(panel.parameters.rowCount()) if panel.parameters.item(i, 0).text() == 'bias.ax')
    panel.parameters.item(row, 1).setText('0.25')
    panel.inspector_scroll.ensureWidgetVisible(panel.apply_button); app.processEvents()
    QTest.mouseClick(panel.apply_button, Qt.LeftButton); app.processEvents()
    assert w.doc.component_graph['components']['component5']['parameters'].get('bias.ax') == .25, panel.status.text()
    assert w.viewport.width() >= 350 and w.height() <= 810
    panel.view.focus_component('component5'); app.processEvents()
    w.grab().save(str(tmp_path/'mixed-graph.png'))


def test_partial_component_results_retain_body_and_derivation_evidence(window, tmp_path):
    from robocad.experiment_results import signals
    app, w, body, run_id = window
    folder = tmp_path/run_id
    write_json(folder/'run.json', {'id': run_id, 'created_at': 0., 'state': 'failed',
        'document_id': w.doc.document_id, 'revision': w.doc.revision, 'error': 'Controller stopped'})
    write_json(folder/'partial.json', {'trace': {'t': [0., .01],
        'signals': {'graph/housing.node.temperature': [300., 300.1]}},
        'signal_units': {'graph/housing.node.temperature': 'K'}, 'evaluation': {'passed': True}})
    mapping = [{'id': 'housing', 'name': 'Housing', 'native_name': 'graph/housing', 'body_id': body}]
    derivations = [{'component_id': 'housing', 'formula': 'volume × density × specific heat'}]
    write_json(folder/'component_graph_mapping.json', mapping)
    write_json(folder/'component_derivations.json', derivations)
    result = w.experiments.partial(run_id)
    assert result['partial'] and 'evaluation' not in result
    assert result['component_derivations'] == derivations
    channel = signals(result)['graph/housing.node.temperature']
    assert channel['node_ids'] == [body] and channel['component_id'] == 'housing'


def test_temperature_plot_reveals_small_changes_with_an_explicit_offset(window):
    from robocad.ui.experiments import TracePlot
    plot = TracePlot()
    plot.set_series({'t': [0., 1.], 'values': [293.15, 293.17], 'unit': 'K'},
        {'t': [0., 1.], 'values': [293.15, 293.18], 'unit': 'K'})
    _, _, lo, hi = plot.bounds()
    assert lo < 293.15 and hi > 293.18
    assert hi-lo < .04  # Padding must not use a percentage of absolute Kelvin.
    assert plot.axis_offset(lo, hi) == 293.15
    plot.set_series({'t': [0., 1.], 'values': [0., 0.], 'unit': 'A'})
    _, _, lo, hi = plot.bounds()
    assert lo < 0. < hi and plot.axis_offset(lo, hi) == 0.
    plot.deleteLater()


@pytest.fixture
def window(tmp_path, monkeypatch):
    app = QApplication.instance() or QApplication([])
    monkeypatch.setattr(ui.MainWindow, 'start_api', lambda self: None)
    monkeypatch.setattr(Document, 'start_autosave', lambda *args: None)
    monkeypatch.setattr(ui, 'Experiments', partial(Experiments, root=tmp_path))
    doc = Document(); ops = Ops(doc)
    part = ops.box((0, 0, 0), (10, 10, 10), name='link')
    w = ui.MainWindow(doc)
    snapshot = capture(doc); run_id = '1'*32; folder = tmp_path/run_id; folder.mkdir()
    (folder/'model.rcad').write_bytes(snapshot.data)
    first = np.eye(4); second = np.eye(4); second[0, 3] = 10.
    result = {'run_id': run_id, 'provenance': {'physical_hash': snapshot.physical_hash},
              'settings': {'step': .001, 'sample': .1}, 'trace': {'t': [0., .1], 'joints': {'hinge': [0., .2]},
                  'poses': {'link': [first.tolist(), second.tolist()]}},
              'cad_mapping': [{'section': 'links', 'name': 'link', 'id': part, 'members': [part]},
                  {'section': 'joints', 'name': 'hinge', 'id': 'joint', 'related_ids': [part]}]}
    write_json(folder/'result.json', result)
    write_json(folder/'run.json', {'id': run_id, 'created_at': 0., 'state': 'completed', 'fraction': 1.,
        'document_id': doc.document_id, 'revision': doc.revision, 'label': 'Recorded run'})
    w.experiments_panel.refresh(); w.experiments_panel.select_run(run_id)
    yield app, w, part, run_id
    doc.dirty = False; w.close(); app.processEvents()
    w.deleteLater(); QCoreApplication.sendPostedEvents(None, QEvent.DeferredDelete)


def test_plot_scrubbing_part_selection_and_live_edits_keep_capture_separate(window):
    app, w, part, run_id = window
    panel = w.experiments_panel; review = panel.review()
    assert review.doc is not w.doc
    assert review.signal.count() == 1
    before = capture(w.doc)
    review.plot.time_selected.emit(.08)
    assert review.slider.value() == 1
    assert review.viewport.pose_matrices[part][0, 3] == 10.
    assert capture(w.doc).data == before.data
    review.part.setCurrentIndex(review.part.findData(part))
    assert review.signal.count() == 1 and review.viewport.selection.nodes() == [part]
    w.ops.transform([part], scale=2.)
    app.processEvents()
    assert review.result['stale'] and 'Live CAD has changed' in review.summary.text()
    assert review.doc.kernel.mass_properties(review.doc.nodes[part].body).volume == pytest.approx(1000.)
    review.note.setPlainText('Inspect the response here'); review.annotate()
    evidence = next(iter(w.doc.annotations.values()))['evidence']
    assert evidence['run_id'] == run_id and evidence['time_range'] == [.1, .1]
    assert evidence['signal'] == 'joints/hinge/angle'
    assert evidence['node_ids'] == [part]
    assert len(w.ops.threads(node_id=part, run_id=run_id)) == 1
    review.close()


def test_joint_flex_patch_editor_converts_units_and_restores_inference(window):
    from PySide6.QtWidgets import QLineEdit
    app, w, beam, _ = window
    base = w.ops.box((-10, -10, -10), (10, 20, 20))
    joint = w.ops.add_joint('revolute', base, beam, (0, 0, 0), (1, 0, 0))
    w.viewport.selection.set_nodes([joint]); w.properties.refresh()
    edit = w.properties.findChild(QLineEdit, 'flex_patch_radius')
    assert float(edit.text()) == pytest.approx(4.55)
    edit.setText('0.8 cm'); edit.editingFinished.emit()
    assert w.doc.nodes[joint].robot['physics']['flex_patch_radius'] == pytest.approx(.008)
    w.properties.refresh()
    edit = w.properties.findChild(QLineEdit, 'flex_patch_radius')
    assert float(edit.text()) == pytest.approx(8.)
    edit.clear(); edit.editingFinished.emit()
    assert w.doc.nodes[joint].robot['physics']['flex_patch_radius'] is None
    w.properties.refresh()
    assert float(w.properties.findChild(QLineEdit, 'flex_patch_radius').text()) == pytest.approx(4.55)


def test_flex_overlay_scrubs_scales_and_keeps_physical_plot_values(window, tmp_path):
    app, w, part, run_id = window
    path = tmp_path/run_id/'result.json'
    result = json.loads(path.read_text())
    result['settings']['flex'] = True
    result['limitations'] = ['Flex replay shows attachment displacement arrows with rigid CAD meshes.']
    result['warnings'] = ['Custom modal normalization is unspecified.']
    result['trace']['flex'] = {'link': [{'name': 'tip', 'point_m': [[.01, .005, .005], [.02, .005, .005]],
        'displacement_m': [[0, 0, 0], [0, 0, -.00002]]}]}
    write_json(path, result)
    review = w.experiments_panel.review()
    try:
        review.show(); app.processEvents()
        review.signal.setCurrentIndex(review.signal.findData('flex/link/0:tip/dz'))
        review.slider.setValue(1)
        assert review.flex_arrows[0]['tip_mm'] == pytest.approx([20, 5, 4.98])
        review.flex_scale.setCurrentIndex(review.flex_scale.findData(100))
        assert review.flex_arrows[0]['tip_mm'] == pytest.approx([20, 5, 3])
        assert review.plot.series['values'][-1] == -.00002
        assert review.plot.series['unit'] == 'm'
        assert 'rigid CAD mesh' in review.flex_scale.currentText()
        assert 'Model limit: Flex replay' in review.metrics.toPlainText()
        assert 'Simulation warning: Custom modal normalization' in review.metrics.toPlainText()
        review.part.setCurrentIndex(review.part.findData(part))
        assert review.signal.findData('flex/link/0:tip/dz') >= 0
        review.slider.setValue(0)
        assert review.flex_arrows[0]['point_mm'] == review.flex_arrows[0]['tip_mm']
        review.slider.setValue(1)
        review.frame_action.click()
        app.processEvents()
        assert review.grab().save(str(tmp_path/'flex-review.png'))
    finally:
        review.close()


def test_automatic_reruns_coalesce_replace_queued_runs_and_stop_when_disabled(window, monkeypatch):
    from PySide6.QtTest import QTest
    app, w, _, _ = window
    panel = w.experiments_panel; manager = w.experiments
    monkeypatch.setattr(manager, '_run', lambda run_id: None)
    original = {job['id'] for job in manager.list()}
    panel.auto.setChecked(True)
    panel.system.setPlainText('let assembly = cad("assembly"); // first edit')
    QTest.qWait(400)
    final_source = 'let assembly = cad("assembly"); // coalesced edit'
    panel.system.setPlainText(final_source)
    QTest.qWait(400)
    assert {job['id'] for job in manager.list()} == original
    QTest.qWait(450)
    first = panel.auto_run_id
    assert first not in original and manager.get(first)['state'] == 'queued'
    assert manager.inputs(first)['system']['files']['system.rhai'] == final_source
    panel.system.appendPlainText('// next candidate')
    QTest.qWait(850)
    second = panel.auto_run_id
    assert second != first and manager.get(first)['state'] == 'cancelled'
    manager._update(second, state='running')
    panel.system.appendPlainText('// keep the running experiment')
    QTest.qWait(850)
    assert panel.auto_run_id != second and manager.get(second)['state'] == 'running'
    before = {job['id'] for job in manager.list()}
    panel.system.appendPlainText('// disabled before the timer fires')
    panel.auto.setChecked(False)
    assert not panel.debounce.isActive()
    QTest.qWait(850)
    assert {job['id'] for job in manager.list()} == before


def test_restoring_inputs_keeps_imported_modules_and_linked_edits(window, tmp_path):
    app, w, part, run_id = window
    panel = w.experiments_panel
    files = {'main.rhai': 'import "parts" as parts;', 'parts.rhai': 'fn value() { 2.0 }'}
    write_json(tmp_path/run_id/'input.json', {'system': {'entry': 'main.rhai', 'files': files},
        'controller': {'sources': {'entry': 'controller.rhai', 'files': {'controller.rhai': 'fn control(t,s,a,state) { a }'}}}, 'settings': {}})
    panel.restore_inputs()
    panel.system.setPlainText('import "parts" as parts; let x = parts::value();')
    captured = panel.source_input('system', panel.system)
    assert captured['files']['parts.rhai'] == files['parts.rhai']
    assert 'let x' in captured['files']['main.rhai']
    path = tmp_path/'external.rhai'; path.write_text('let x = 1;')
    panel.linked['system'] = {'path': path, 'text': None}; panel.poll_sources()
    assert panel.system.isReadOnly() and panel.system.toPlainText() == 'let x = 1;'
    path.write_text('let x = 2;'); panel.poll_sources()
    assert panel.system.toPlainText() == 'let x = 2;'


def test_candidate_review_accepts_one_undoable_change(window):
    from robocad.ui.experiments import CandidateReview
    app, w, part, _ = window
    w.candidates.create({'expected_revision': w.doc.revision, 'label': 'Scaled part', 'operations': [
        {'op': 'transform', 'args': [[part]], 'kwargs': {'scale': 2}}]})
    review = CandidateReview(w.experiments_panel)
    assert 'geometry changed' in review.changes.toPlainText()
    assert review.doc is not w.doc and review.accept_button.isEnabled()
    before = len(w.ops.stack.undo_stack)
    review.accept_candidate()
    assert len(w.ops.stack.undo_stack) == before + 1
    assert w.doc.kernel.mass_properties(w.doc.nodes[part].body).volume == pytest.approx(8000.)
    w.ops.undo()
    assert w.doc.kernel.mass_properties(w.doc.nodes[part].body).volume == pytest.approx(1000.)
    review.close()


def test_restoring_process_backend_and_registry_discovery_keep_authored_inputs(window, tmp_path):
    app, w, part, run_id = window
    panel = w.experiments_panel
    bundle = {'runtime': 'python', 'entry': 'main.py', 'files': {'main.py': 'print(1)'}, 'arguments': []}
    graph = {'version': 1, 'components': {'disk': {'id': 'disk', 'type': 'rotational.inertia',
        'name': 'Captured disk', 'body_id': part, 'parameters': {'inertia': 2.}}}, 'connections': {}}
    write_json(tmp_path/run_id/'input.json', {'system': {'entry': 's.rhai', 'files': {'s.rhai': ''}},
        'controller': {'language': 'process', 'process': bundle, 'parameters': {'gain': 2}, 'interface': 'driver_duty'},
        'profile': 'validation', 'seed': 17, 'settings': {}, 'component_graph': graph})
    panel.restore_inputs()
    assert w.doc.component_graph == graph
    w.ops.undo(); assert not w.doc.component_graph['components']
    w.ops.redo(); assert w.doc.component_graph == graph
    request = panel.request()
    assert request['controller']['language'] == 'process' and request['controller']['process'] == bundle
    assert request['profile'] == 'validation' and request['seed'] == 17
    assert request['controller']['interface'] == 'driver_duty'
    panel.receive_catalogue([{'type': 'rotational.inertia', 'name': 'Inertia', 'parameters_complete': True,
        'ports': [{'name': 'shaft', 'schema': {'Acausal': 'Rotational'}, 'direction': 'acausal', 'lanes': [{'across': 'angle', 'across_unit': 'rad', 'through': 'torque', 'through_unit': 'N·m'}]}],
        'parameters': [{'name': 'inertia', 'unit': 'kg·m²', 'required': True, 'default': None}]}])
    assert 'kg·m²' in panel.component_details.toPlainText() and 'angle [rad]' in panel.component_details.toPlainText()
