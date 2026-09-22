"""Isolated CAD-window startup. Progress is JSON lines; source CAD is read-only.

The owning UI authorizes the final handoff with `show`. Once handed off, this
process owns its window independently of the launcher, including unsaved edits.
"""
import json
import os
import sys
import time

_control = None


def report(**message):
    print(json.dumps(message), file=_control, flush=True)


def main():
    global _control
    # Keep the JSON handshake on its own pipe. All native/Python output can go
    # straight to the session log from the beginning, including load failures.
    _control = os.fdopen(os.dup(1), 'w', buffering=1)
    from ..diagnostics import start, event, log
    path = sys.argv[1]
    session = start('editor', model_path=path)
    report(event='diagnostics', log_path=session['log_path'])
    from .model_loading import prepare_model
    cache_root = sys.argv[2] or None
    try:
        doc, items, stats = prepare_model(path,
            lambda stage, done, total, name: report(event='progress', stage=stage, done=done, total=total, name=name),
            cache_root=cache_root)
        report(event='prepared')
        # No editable window exists before this handshake. Cancellation may
        # terminate any native geometry call up to this point without losing edits.
        if sys.stdin.readline().strip() != 'show':
            return 0
        from PySide6.QtWidgets import QApplication
        from .app import MainWindow
        app = QApplication([])
        app.setApplicationName('robocad')
        window_started = time.perf_counter()
        window = MainWindow(doc=doc, prepared=items)
        if '--imports-json' in sys.argv:
            for import_path in json.loads(sys.argv[sys.argv.index('--imports-json') + 1]):
                window.import_path(import_path)
        if '--benchmark' in sys.argv:
            window.setEnabled(False)
        window.show()
        window.raise_(); window.activateWindow()
        stats.update(window_seconds=time.perf_counter()-window_started,
                     api_url=window.api.url if window.api else None,
                     pid=os.getpid(), log_path=session['log_path'])
        window.load_stats = stats
        window.status(f"Opened · {stats['cache_hits']} / {stats['parts']} display parts reused")
        report(event='ready', stats=stats)
        event('editor_ready', **stats)
        _control.close()
        if '--benchmark' in sys.argv:
            from PySide6.QtCore import QTimer
            QTimer.singleShot(200, app.quit)
        # stdout/stderr already belong to the durable log, not the launcher.
        with open(os.devnull, 'rb') as sink:
            os.dup2(sink.fileno(), 0)
        return app.exec()
    except Exception as error:
        log.exception('Editor startup/event loop failed')
        if not _control.closed:
            report(event='failed', error=f'{error}\nDiagnostics: {session["log_path"]}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
