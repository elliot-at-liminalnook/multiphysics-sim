"""Profile-only reuse must reject every physical/archive change."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('export_profile_revision', Path(__file__).parents[1] / 'scripts/export_profile_revision.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ProfileRevisionTests(unittest.TestCase):
    def test_only_profile_and_save_metadata_may_change(self):
        old = {'saved': 1, 'revision': 1, 'nodes': [{'mass': 1}],
               'robot_settings': {'world': {'gravity': 9.81}, 'actuator_profiles': {'version': 1}}}
        new = copy.deepcopy(old)
        new.update(saved=2, revision=2)
        new['robot_settings']['actuator_profiles'] = {'version': 2}
        with tempfile.TemporaryDirectory() as folder:
            a, b = Path(folder)/'a.rcad', Path(folder)/'b.rcad'
            def write(path, manifest, geometry=b'original', extra=False):
                with zipfile.ZipFile(path, 'w') as z:
                    z.writestr('manifest.json', json.dumps(manifest))
                    z.writestr('brep/body', geometry)
                    if extra:
                        z.writestr('unexpected', b'new')
            write(a, old)
            write(b, new)
            self.assertEqual(module.verify_profile_only(a, b), {'version': 2})
            physical = copy.deepcopy(new)
            physical['nodes'][0]['mass'] = 2
            for manifest, geometry, extra in [(physical, b'original', False),
                    (new, b'modified', False), (new, b'original', True)]:
                write(b, manifest, geometry, extra)
                with self.assertRaises(ValueError):
                    module.verify_profile_only(a, b)


if __name__ == '__main__':
    unittest.main()
