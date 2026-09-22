"""Measure a real desktop load: cold/warm time, UI timer gaps and window handoff.

Run from cad: .venv/bin/python scripts/benchmark_model_load.py model.rcad --cold
The cold run uses a temporary cache and never changes the source CAD archive.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import QTimer
from PySide6.QtWidgets import QApplication
from robocad.ui.app import DARK_QSS
from robocad.ui.model_loading import ModelLoadDialog


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('path', type=Path)
    parser.add_argument('--cold', action='store_true')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--max-load-seconds', type=float, default=65)
    parser.add_argument('--max-ui-gap-ms', type=float, default=150)
    args = parser.parse_args()
    digest = hashlib.sha256(args.path.read_bytes()).hexdigest()
    folder = tempfile.TemporaryDirectory(prefix='robocad-benchmark-') if args.cold else None
    app = QApplication([])
    started = last = time.perf_counter()
    gaps = []
    long_gaps = []
    output = {}
    timer = QTimer(); timer.setInterval(16)
    def tick():
        nonlocal last
        now = time.perf_counter(); gap = 1000*(now-last); gaps.append(gap); last = now
        if gap > 50:
            long_gaps.append({'elapsed_seconds': now-started, 'gap_ms': gap, 'stage': dialog.job['stage']})
    timer.timeout.connect(tick); timer.start()
    def ready(stats):
        timer.stop()
        ordered = sorted(gaps)
        output.update(stats, source_sha256=digest, cold=args.cold,
                      total_seconds=time.perf_counter()-started,
                      ui_ticks=len(gaps), max_ui_gap_ms=max(gaps, default=0),
                      long_gaps=long_gaps,
                      p99_ui_gap_ms=ordered[min(len(ordered)-1, int(len(ordered)*.99))] if ordered else 0)
        output['source_unchanged'] = digest == hashlib.sha256(args.path.read_bytes()).hexdigest()
        output['passed'] = (output['total_seconds'] <= args.max_load_seconds
                            and output['max_ui_gap_ms'] <= args.max_ui_gap_ms
                            and output['source_unchanged'])
        text = json.dumps(output, indent=2)
        print(text, flush=True)
        if args.out:
            args.out.parent.mkdir(parents=True, exist_ok=True)
            args.out.write_text(text+'\n')
        QTimer.singleShot(0, app.quit)
    dialog = ModelLoadDialog(args.path.resolve(), ready, folder.name if folder else None, benchmark=True)
    dialog.setStyleSheet(DARK_QSS)
    # Report dialog creation separately; measure event gaps once it is visible.
    output['dialog_seconds'] = time.perf_counter() - started
    last = time.perf_counter(); gaps.clear()
    app.exec()
    if folder:
        folder.cleanup()
    return 0 if output.get('passed') else 1


if __name__ == '__main__':
    raise SystemExit(main())
