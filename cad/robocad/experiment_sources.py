"""Authoritative linked Rhai bundle capture shared by Qt and headless clients.

Paths refer to the service host filesystem. Capture all sibling-tree .rhai
modules as the reference UI did; reads never write linked files.
"""
from pathlib import Path
from .experiments import sources
from .kernel import KernelError


def linked_sources(path):
    path = Path(path).expanduser().resolve()
    if path.suffix != '.rhai' or not path.is_file(): raise KernelError('Link an existing .rhai entry on the service host')
    files = {str(p.relative_to(path.parent)): p.read_text()
             for p in sorted(path.parent.rglob('*.rhai')) if p.is_file()}
    return sources({'entry': path.name, 'files': files}, path.name, '')
