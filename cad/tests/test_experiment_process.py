import base64
import pytest
from robocad.experiment_config import request
from robocad.experiment_process import capture
from robocad.kernel import KernelError


def test_captured_process_bundle_retains_sources_and_round_trips(tmp_path):
    path = tmp_path/'helper.py'; path.write_text('VERSION=1')
    value = {'language': 'process', 'process': {'runtime': 'python', 'entry': 'main.py',
             'files': {'main.py': 'import helper', 'helper.py': {'path': str(path)}}}}
    first = request({'controller': value})
    path.write_text('VERSION=2')
    assert base64.b64decode(first['controller']['process']['files']['helper.py']['base64']) == b'VERSION=1'
    assert request(first) == first
    corrupted = first['controller']['process']['files']['helper.py']
    corrupted['base64'] = base64.b64encode(b'changed').decode()
    with pytest.raises(KernelError, match='content check'): request(first)


@pytest.mark.parametrize('files', [
    {'../main.py': 'x'}, {'/main.py': 'x'}, {'main.py': 'x', 'dir': 'x', 'dir/file': 'x'},
    {'main.py': {'base64': 'invalid*'}}, {'main.py': {'unknown': 'x'}},
])
def test_process_bundles_reject_invalid_artifacts(files):
    with pytest.raises(KernelError): capture({'runtime': 'python', 'entry': next(iter(files)), 'files': files})
