"""Content-checked intermediate CAD artifacts with explicit dependency keys."""
import json
from pathlib import Path
import time
from .snapshots import canonical, digest
from .experiments import write_json


class DerivationCache:
    def __init__(self, root, identity):
        self.root = Path(root)/digest(canonical(identity))
        self.stats = {}

    def get(self, stage, dependencies, build):
        started = time.perf_counter()
        key = digest(canonical(dependencies))
        path = self.root/stage/(key+'.json')
        stats = self.stats.setdefault(stage, {'hits': 0, 'misses': 0, 'seconds': 0.})
        try:
            artifact = json.loads(path.read_bytes())
            if artifact['key'] == key and artifact['content_hash'] == digest(canonical(artifact['value'])):
                stats['hits'] += 1
                return artifact['value']
        except (OSError, KeyError, ValueError, TypeError): pass
        finally: stats['seconds'] += time.perf_counter()-started
        started = time.perf_counter()
        stats['misses'] += 1
        value = json.loads(canonical(build()))
        path.parent.mkdir(parents=True, exist_ok=True)
        write_json(path, {'key': key, 'dependencies': dependencies, 'content_hash': digest(canonical(value)), 'value': value})
        stats['seconds'] += time.perf_counter()-started
        return value
