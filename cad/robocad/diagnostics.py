"""Durable desktop diagnostics, independent of the terminal/launcher lifetime.

Run ``python -m robocad.diagnostics`` to collect matching macOS crash reports
and print the latest editor session. No geometry or comment bodies are logged.
"""
import atexit
from datetime import datetime, timezone
import faulthandler
import signal
import json
import logging
import os
from pathlib import Path
import platform
import shutil
import sys
import threading

_session = None
_stream = None
log = logging.getLogger('robocad')


def log_root():
    override = os.environ.get('ROBOCAD_LOG_DIR')
    if override:
        return Path(override).expanduser()
    if sys.platform == 'darwin':
        return Path.home() / 'Library/Logs/RoboCAD'
    if sys.platform == 'win32':
        return Path(os.environ.get('LOCALAPPDATA', str(Path.home()))) / 'RoboCAD/Logs'
    return Path(os.environ.get('XDG_STATE_HOME', str(Path.home() / '.local/state'))) / 'robocad/logs'


def _now():
    return datetime.now(timezone.utc).isoformat()


def _write_json(path, value):
    temp = path.with_name(path.name + f'.{os.getpid()}.tmp')
    temp.write_text(json.dumps(value, indent=2) + '\n')
    temp.replace(path)


def event(name, **fields):
    if _session is not None:
        log.info('%s %s', name, json.dumps(fields, default=str))


def session_info():
    return dict(_session) if _session else None


def start(role='editor', *, model_path=None):
    """Install once per process, before Qt/native imports when possible.

    Redirect OS descriptors as well as Python streams: Qt, OCCT and Objective-C
    diagnostics write directly to stderr. faulthandler keeps a separate open
    descriptor so a fatal signal remains reportable after launcher handoff.
    """
    global _session, _stream
    if _session is not None:
        return session_info()
    root = log_root()
    root.mkdir(parents=True, exist_ok=True)
    folder = root / (datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ') + f'-{role}-{os.getpid()}')
    folder.mkdir(mode=0o700)
    _stream = open(folder / 'session.log', 'a', buffering=1)
    os.chmod(folder / 'session.log', 0o600)
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.flush()
        except (OSError, ValueError):
            pass
    os.dup2(_stream.fileno(), 1)
    os.dup2(_stream.fileno(), 2)
    # Keep print output through abrupt native crashes, not only orderly exit.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, 'reconfigure'):
            stream.reconfigure(line_buffering=True, write_through=True)
    handler = logging.StreamHandler(_stream)
    handler.setFormatter(logging.Formatter('%(asctime)s %(levelname)s %(message)s'))
    log.addHandler(handler)
    log.setLevel(logging.INFO)
    log.propagate = False
    faulthandler.enable(file=_stream, all_threads=True)
    # `kill -USR1 <pid>` writes every thread's Python stack to session.log
    # without stopping the app: the way to see what a hung UI thread is doing.
    if hasattr(signal, 'SIGUSR1'):
        faulthandler.register(signal.SIGUSR1, file=_stream, all_threads=True)
    _session = dict(pid=os.getpid(), parent_pid=os.getppid(), role=role,
                    started_at=_now(), folder=str(folder), log_path=str(folder / 'session.log'),
                    model_path=str(model_path) if model_path else None,
                    python=sys.version, platform=platform.platform(),
                    executable=sys.executable, state='started')
    _write_json(folder / 'session.json', _session)
    _write_json(root / 'latest.json', _session)
    _write_json(root / f'latest-{role}.json', _session)
    def exception_hook(kind, value, tb):
        log.error('Uncaught Python exception', exc_info=(kind, value, tb))
    sys.excepthook = exception_hook
    threading.excepthook = lambda args: exception_hook(args.exc_type, args.exc_value, args.exc_traceback)
    atexit.register(_finish)
    event('session_started', **_session)
    # macOS writes .ips reports asynchronously, often after the app has exited.
    # Collect previous sessions on the next launch without blocking the UI.
    if sys.platform == 'darwin':
        threading.Thread(target=collect_native_reports, daemon=True, name='crash-report-collector').start()
    return session_info()


def _finish():
    if _session:
        event('process_exit')
        _session.update(state='exited', ended_at=_now())
        _write_json(Path(_session['folder']) / 'session.json', _session)
        _stream.flush()


def collect_native_reports(root=None, reports=None):
    """Copy only reports matching our recorded PID and process start time.

    Other Python applications' reports must not be mistaken for RoboCAD crashes.
    Reports remain in Apple's original DiagnosticReports directory as well.
    """
    root = Path(root) if root else log_root()
    reports = Path(reports) if reports else Path.home() / 'Library/Logs/DiagnosticReports'
    sessions = {}
    for path in root.glob('*/session.json'):
        try:
            data = json.loads(path.read_text())
            sessions.setdefault(data['pid'], []).append((path.parent, data))
        except (OSError, ValueError, KeyError):
            continue
    copied = []
    for report in reports.glob('*.ips'):
        try:
            if report.stat().st_size > 20 * 1024 * 1024:
                continue
            text = report.read_text()
            data = json.loads(text.split('\n', 1)[1])
            candidates = sessions.get(data.get('pid'), [])
            if not candidates:
                continue
            launched = datetime.fromisoformat(data['procLaunch'])
            for folder, session in candidates:
                started = datetime.fromisoformat(session['started_at'])
                if abs((started - launched).total_seconds()) > 60:
                    continue
                target = folder / report.name
                if not target.exists():
                    temp = target.with_name(target.name + f'.{os.getpid()}.tmp')
                    shutil.copy2(report, temp)
                    temp.replace(target)
                    copied.append(str(target))
                break
        except (OSError, ValueError, KeyError, IndexError, TypeError):
            continue
    return copied


def main():
    root = log_root()
    collect_native_reports()
    print(root)
    latest = root / 'latest-editor.json'
    if latest.exists():
        print(json.loads(latest.read_text())['log_path'])


if __name__ == '__main__':
    main()
