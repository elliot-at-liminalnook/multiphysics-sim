import json
import urllib.request

import pytest

from robocad.api import ApiServer
from robocad.client import RoboClient
from robocad.document import Document
from robocad.experiments import Experiments


def test_experiment_and_candidate_routes_share_revisions_and_captured_inputs(tmp_path, monkeypatch):
    binary = tmp_path/'fake-runner'; binary.write_bytes(b'transport-test')
    doc = Document(); manager = Experiments(doc, root=tmp_path, binary=binary)
    # Transport/transaction test; real worker execution is covered separately.
    monkeypatch.setattr(manager, '_run', lambda run_id: None)
    server = ApiServer(doc, port=0); server.service._experiments = manager; server.start()
    client = RoboClient(server.url)
    try:
        revision = client.get('/doc')['revision']
        req = urllib.request.Request(server.url+'/experiments', method='POST',
            data=json.dumps({'expected_revision': revision}).encode(), headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(req) as response:
            assert response.status == 202
            job = json.load(response)
        assert client.get(f"/experiments/{job['id']}/inputs")['provenance']['revision'] == revision
        assert client.get('/experiments')[0]['id'] == job['id']
        assert client.post(f"/experiments/{job['id']}/cancel")['state'] == 'cancelled'
        with pytest.raises(RuntimeError, match='409'):
            client.post('/experiments', {'expected_revision': revision-1})
        batch = client.post('/doc/batch', {'expected_revision': revision, 'operations': [
            {'op': 'box', 'args': [[0,0,0],[10,10,10]], 'as': 'part'}]})
        part = batch['results']['part']
        candidate = client.post('/candidates', {'expected_revision': batch['revision'], 'operations': [
            {'op': 'rename', 'args': [part, 'Candidate name']}]})
        assert client.get(f"/nodes/{part}")['name'] != 'Candidate name'
        run = client.post(f"/candidates/{candidate['id']}/experiments", {'expected_revision': candidate['revision']})
        inputs = client.get(f"/experiments/{run['id']}/inputs")
        assert inputs['provenance']['candidate_id'] == candidate['id']
        accepted = client.post(f"/candidates/{candidate['id']}/accept", {'expected_revision': batch['revision']})
        assert accepted['state'] == 'accepted'
        assert client.get(f"/nodes/{part}")['name'] == 'Candidate name'
        client.undo()
        assert client.get(f"/nodes/{part}")['name'] != 'Candidate name'
    finally:
        server.stop()
