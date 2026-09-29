"""The print registry is one source: CAD reads the same file and fingerprint
as the Rust tools, and CAD print materials take their values from it."""
import json
import os
import subprocess

import pytest

from robocad import print_registry as pr
from robocad.document import Document

BIN = os.path.join(pr.REPO, 'target', 'release', 'sim-print')


def test_registry_values_flow_into_cad_materials():
    reg, sha = pr.load()
    pla = reg['materials']['pla-basic']
    props = Document().materials['pla'].props()
    assert props['youngs_modulus'] == pla['modulus_in_layer']['value']
    assert props['print']['registry_material'] == 'pla-basic' and props['print']['registry_sha256'] == sha
    assert props['print']['anisotropy_z'] == pytest.approx(pla['modulus_across_layers']['value'] / pla['modulus_in_layer']['value'])
    # Non-print materials keep their own values.
    assert Document().materials['steel'].props()['print'] is None


def test_usable_box_and_joint_values():
    x, y, z = pr.usable_mm('bambu-a1-mini')
    assert (x, y, z) == (176, 176, 180)
    assert pr.insert('M3')['hole_mm'] == 4.0
    assert pr.joint('dovetail.angle_deg') == 15
    with pytest.raises(KeyError, match='not in the print registry'):
        pr.printer('no-such-printer')


@pytest.mark.skipif(not os.path.isfile(BIN), reason='build sim-print first')
def test_rust_reads_the_same_file_and_fingerprint():
    _, sha = pr.load()
    out = subprocess.run([BIN, 'registry', pr.default_path()], capture_output=True, text=True, check=True)
    assert json.loads(out.stdout)['sha256'] == sha
