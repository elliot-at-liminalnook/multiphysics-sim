import pytest
from robocad.experiment_config import request, cad_overrides
from robocad.experiments import Experiments
from robocad.kernel import KernelError


@pytest.mark.parametrize('invalid', [
    {'setings': {}}, {'seed': True}, {'seed': -1}, {'seed': 2**53}, {'preflight': 'true'},
    {'profile': 'fast'}, {'parameters': []}, {'settings': {'contact': 1}},
    {'settings': {'step': 0}}, {'settings': {'seconds': float('nan')}},
    {'settings': {'step': .2, 'sample': .1}}, {'settings': {'stepp': .1}},
    {'controller': {'language': 'rhai', 'commmand': []}},
    {'controller': {'language': 'rhai', 'parameters': []}},
    {'controller': {'language': 'process', 'command': 'python controller.py'}},
])
def test_invalid_inputs_fail_before_snapshot_or_job_creation(tmp_path, invalid):
    manager = Experiments(root=tmp_path)
    with pytest.raises(KernelError): manager.create(invalid)
    assert not list(tmp_path.iterdir())


def test_profiles_and_explicit_overrides_are_captured():
    quick = request({})
    validation = request({'profile': 'validation', 'seed': 125, 'settings': {'contact': False}})
    assert quick['seed'] == 0 and not quick['settings']['noise']
    assert validation['settings']['step'] < quick['settings']['step']
    assert validation['settings']['flex'] and validation['settings']['noise']
    assert not validation['settings']['contact'] and validation['seed'] == 125


def test_cad_overrides_record_defaults_ids_arrays_and_sources():
    model = {'links': [{'id': 'stable', 'name': 'rod', 'mass': 1., 'com': [0., 0., .2]}]}
    location = {'source': 'scenario.rhai', 'line': 12, 'column': 3}
    overrides = [{'section': 'links', 'id': 'rod', 'field': '/com/2', 'value': .3}]
    evidence = cad_overrides(model, overrides, location)
    assert model['links'][0]['com'][2] == .3
    assert evidence[0]['id'] == 'stable' and evidence[0]['before'] == .2
    assert evidence[0]['source'] == location
    for field, value in [('/id', 1), ('/com/4', .1), ('/mass', True), ('/mass', float('inf'))]:
        with pytest.raises(KernelError):
            cad_overrides(model, [{'section': 'links', 'id': 'stable', 'field': field, 'value': value}])
    with pytest.raises(KernelError, match='Conflicting'):
        cad_overrides(model, overrides + [{**overrides[0], 'id': 'stable'}])


def test_flex_patch_overrides_require_rederivation_instead_of_changing_only_metadata():
    model = {'joints': [{'id': 'j', 'name': 'hinge', 'physics': {'flex_patch_radius': .004}}],
        'links': [{'id': 'l', 'name': 'link', 'flex': {'boundary_frames': [{'patch': {'radius_m': .004}}]}}]}
    for section, identity, field in [('joints', 'j', '/physics/flex_patch_radius'),
                                    ('links', 'l', '/flex/boundary_frames/0/patch/radius_m')]:
        with pytest.raises(KernelError, match='before CAD derivation'):
            cad_overrides(model, [{'section': section, 'id': identity, 'field': field, 'value': .008}])
    assert model['joints'][0]['physics']['flex_patch_radius'] == .004
    assert model['links'][0]['flex']['boundary_frames'][0]['patch']['radius_m'] == .004
