from copy import deepcopy
import io
import pytest
from robocad.commands import Ops
from robocad.document import Document
from robocad.component_graph import empty_graph
from robocad.candidates import Candidates
from robocad.kernel import KernelError
from robocad.snapshots import capture


def thermal_graph(body):
    graph = empty_graph()
    graph['components']['capacity'] = {'id': 'capacity', 'name': 'Housing heat capacity',
        'type': 'thermal.capacitance', 'body_id': body,
        'parameters': {'heat_capacity': 20., 'initial.temperature': 300.}}
    graph['components']['heater'] = {'id': 'heater', 'name': 'Winding heat',
        'type': 'thermal.heat_source', 'body_id': body, 'parameters': {'power': 10.}}
    graph['connections']['heat'] = {'id': 'heat', 'ports': [
        {'component_id': 'capacity', 'port': 'node'}, {'component_id': 'heater', 'port': 'node'}]}
    return graph


def test_graph_snapshot_candidate_acceptance_and_undo(tmp_path):
    doc = Document(); ops = Ops(doc)
    body = ops.box((0, 0, 0), (10, 10, 10))
    before = capture(doc)
    graph = thermal_graph(body)
    candidates = Candidates(doc, ops, tmp_path)
    candidate = candidates.create({'expected_revision': doc.revision,
        'operations': [{'op': 'set_component_graph', 'args': [graph]}]})
    assert doc.component_graph == empty_graph()
    assert candidate['changes']['physical_changed']
    assert candidate['changes']['document']['component_graph']['after'] == graph
    candidates.accept(candidate['id'], doc.revision)
    accepted = capture(doc)
    assert accepted.physical_hash != before.physical_hash
    assert Document.load(io.BytesIO(accepted.data)).component_graph == graph
    ops.undo()
    assert capture(doc).physical_hash == before.physical_hash
    ops.redo()
    assert doc.component_graph == graph


def test_invalid_graph_is_atomic_and_does_not_change_revision():
    doc = Document(); ops = Ops(doc)
    body = ops.box((0, 0, 0), (10, 10, 10))
    graph = thermal_graph(body)
    ops.set_component_graph(graph)
    before = capture(doc)
    for mutation in ('missing_body', 'missing_component', 'duplicate_port', 'nan'):
        invalid = deepcopy(graph)
        if mutation == 'missing_body': invalid['components']['heater']['body_id'] = 'missing'
        if mutation == 'missing_component': del invalid['components']['heater']
        if mutation == 'duplicate_port': invalid['connections']['heat']['ports'].append(invalid['connections']['heat']['ports'][0])
        if mutation == 'nan': invalid['components']['heater']['parameters']['power'] = float('nan')
        with pytest.raises(KernelError): ops.set_component_graph(invalid)
        assert capture(doc).data == before.data


def test_system_graph_rest_and_python_client_share_revision_checks():
    from robocad.api import ApiServer
    from robocad.client import RoboClient
    doc = Document(); ops = Ops(doc)
    body = ops.box((0, 0, 0), (10, 10, 10))
    server = ApiServer(doc, port=0); server.start()
    client = RoboClient(server.url)
    try:
        original = client.system_graph()
        graph = thermal_graph(body)
        changed = client.set_system_graph(graph, original['revision'])
        assert changed['revision'] > original['revision']
        assert client.system_graph()['graph'] == graph
        with pytest.raises(RuntimeError, match='409'):
            client.set_system_graph(empty_graph(), original['revision'])
        assert doc.component_graph == graph
    finally:
        server.stop()


def test_native_catalogue_crud_checks_units_and_merges_connection_nodes(tmp_path):
    from pathlib import Path
    from robocad.api import ApiServer
    from robocad.client import RoboClient
    from robocad.experiments import Experiments
    doc = Document(); manager = Experiments(doc, root=tmp_path)
    if not Path(manager.binary).exists(): pytest.skip('Build the native runner for registry-driven API acceptance')
    server = ApiServer(doc, port=0); server.service._experiments = manager; server.start()
    client = RoboClient(server.url)
    def add(kind, name, parameters):
        return client.post('/system/components', {'expected_revision': doc.revision,
            'component': {'type': kind, 'name': name, 'parameters': parameters}})['id']
    def connect(*ports):
        return client.post('/system/connections', {'expected_revision': doc.revision,
            'ports': [{'component_id': identity, 'port': name} for identity, name in ports]})
    try:
        from robocad.component_graph import RegistryView
        catalogue = client.get('/experiments/catalogue')
        assert len(catalogue) == 117
        assert all(component['parameters_complete'] for component in catalogue)
        registry = RegistryView(catalogue)
        robot = {'id': 'robot', 'name': 'Assembly', 'type': 'robot.articulated',
                 'parameters': {'imu.zulu.ax': 0, 'imu.alpha.ax': 0, 'imu.alpha.gx': 0}}
        channels = {port['name']: port for port in registry.ports(robot)}
        assert channels['imu.alpha.ax']['unit'] == 'm/s²'
        assert channels['imu.alpha.gx']['unit'] == 'rad/s'
        assert [name for name in channels if name.startswith('imu.')] == ['imu.alpha.ax', 'imu.zulu.ax', 'imu.alpha.gx']
        robot['binding'] = 'cad/assembly'
        graph = {'components': {'robot': robot}}
        assert registry.port(graph, {'component_id': 'robot', 'port': 'imu.beta.gz'})['unit'] == 'rad/s'
        for invalid in ['imu.beta.bad', 'imu..ax']:
            with pytest.raises(KernelError, match='not a declared port'):
                registry.port(graph, {'component_id': 'robot', 'port': invalid})
        capacity = add('thermal.capacitance', 'Capacity', {'heat_capacity': 20})
        heater = add('thermal.heat_source', 'Heat', {'power': 10})
        second = add('thermal.heat_source', 'More heat', {'power': 2})
        bad = add('rotational.ground', 'Ground', {})
        before = doc.revision
        with pytest.raises(RuntimeError, match='incompatible'):
            connect((capacity, 'node'), (bad, 'flange'))
        assert doc.revision == before and not doc.component_graph['connections']
        initial_connection = connect((capacity, 'node'), (heater, 'node'))['id']
        connect((heater, 'node'), (second, 'node'))
        assert len(doc.component_graph['connections']) == 1
        assert initial_connection in doc.component_graph['connections']
        assert len(next(iter(doc.component_graph['connections'].values()))['ports']) == 3
        with pytest.raises(RuntimeError, match='declared range'):
            client.patch('/system/components/'+capacity, {'expected_revision': doc.revision,
                'component': {'parameters': {'heat_capacity': -1}}})
        client.delete('/system/components/'+second+'?expected_revision='+str(doc.revision))
        assert second not in client.get('/system/components')
        remaining = client.get('/system/connections')
        assert len(remaining) == 1
        assert {p['component_id'] for p in next(iter(remaining.values()))['ports']} == {capacity, heater}
    finally:
        server.stop()
