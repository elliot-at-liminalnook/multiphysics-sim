"""Isolated exact geometry measurements for the desktop selection inspector.

OCCT calls can hold Python's GIL, so a Python thread cannot keep Qt responsive.
Only an explicit measurement request serializes the selected B-reps here.
"""
import json
import sys
from pathlib import Path

from .analysis import selection_properties
from .document import Document, Material, Node


def measure(root):
    root = Path(root)
    doc = Document()
    ids = []
    for index, entry in enumerate(json.loads((root / 'input.json').read_text())):
        nid = str(index)
        doc.materials[nid] = Material(nid, 'Measurement material', entry['density'])
        body = doc.kernel.deserialize((root / entry['file']).read_bytes(), entry['kind'])
        doc.nodes[nid] = Node(id=nid, name=nid, kind='body', body=body, material=nid)
        ids.append(nid)
    result = selection_properties(doc, ids)
    if result is None:
        raise ValueError('No measurable geometry selected')
    return {key: getattr(result, key) for key in ('size', 'volume', 'area', 'mass_g', 'centroid')}


if __name__ == '__main__':
    print(json.dumps(measure(sys.argv[1])))
