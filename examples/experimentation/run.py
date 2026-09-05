"""Run the same captured experiments as the desktop and REST service."""
import argparse
import json
from pathlib import Path
import time

from build_model import ROOT, build
from robocad.experiments import Experiments, TERMINAL

HERE = Path(__file__).resolve().parent


def request(doc):
    files = {p.name: p.read_text() for p in HERE.glob('*.rhai')}
    return {'expected_revision': doc.revision, 'parameters': {'target1': .2},
            'system': {'entry': 'system.rhai', 'files': files},
            'controller': {'language': 'rhai', 'sources': {'entry': 'controller.rhai', 'files': files},
                           'parameters': {'target1': .2, 'target2': -.15}}}


def wait(manager, job, timeout=60):
    start = time.monotonic()
    while manager.get(job['id'])['state'] not in TERMINAL:
        if time.monotonic()-start > timeout:
            manager.cancel(job['id'])
            raise RuntimeError(f"Run {job['id']} exceeded {timeout} seconds; cancellation requested")
        time.sleep(.01)
    record = manager.get(job['id'])
    if record['state'] != 'completed': raise RuntimeError(json.dumps(manager.diagnostics(job['id'])))
    return manager.result(job['id'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--two-joint', action='store_true')
    parser.add_argument('--output', type=Path, default=ROOT/'runs'/'experimentation')
    args = parser.parse_args()
    doc, _ = build(args.two_joint)
    manager = Experiments(doc, root=args.output)
    job = manager.create(request(doc))
    print(f"Queued {job['id']}", flush=True)
    try: result = wait(manager, job)
    except KeyboardInterrupt:
        manager.cancel(job['id']); raise
    print(json.dumps({k: result.get(k) for k in ('run_id', 'evaluation', 'timing', 'cache')}, indent=2))
    if result['evaluation']['status'] == 'failed': raise SystemExit(1)


if __name__ == '__main__': main()
