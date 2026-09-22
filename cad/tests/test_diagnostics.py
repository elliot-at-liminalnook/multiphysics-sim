"""Exercise real process exit/signal paths without crashing the test runner."""
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

import pytest

from robocad.diagnostics import collect_native_reports


def run_session(tmp_path, code):
    env = dict(os.environ, ROBOCAD_LOG_DIR=str(tmp_path))
    env['PYTHONPATH'] = str(Path(__file__).resolve().parents[1])
    return subprocess.run([sys.executable, '-u', '-c',
        'from robocad.diagnostics import start, event\nstart()\n' + code],
        env=env, capture_output=True, timeout=15)


def latest(tmp_path):
    data = json.loads((tmp_path / 'latest-editor.json').read_text())
    return data, Path(data['log_path']).read_text()


def test_process_streams_exceptions_and_exit_are_durable(tmp_path):
    result = run_session(tmp_path, '''import os, threading
print('python stdout')
os.write(2, b'native stderr\\n')
def worker(): raise RuntimeError('thread failure')
t=threading.Thread(target=worker); t.start(); t.join()
raise ValueError('uncaught failure')
''')
    assert result.returncode != 0
    info, text = latest(tmp_path)
    assert result.stdout == b'' and result.stderr == b''
    assert all(value in text for value in ('python stdout', 'native stderr',
        'RuntimeError: thread failure', 'ValueError: uncaught failure', 'process_exit'))
    assert json.loads((Path(info['folder']) / 'session.json').read_text())['state'] == 'exited'


@pytest.mark.skipif(os.name == 'nt', reason='POSIX fatal signal regression')
def test_native_crash_records_all_thread_tracebacks(tmp_path):
    result = run_session(tmp_path, '''import os, signal, resource
resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
def native_crash_probe():
    event('before_test_crash', node_id='test-part')
    os.kill(os.getpid(), signal.SIGABRT)
native_crash_probe()
''')
    assert result.returncode == -signal.SIGABRT
    info, text = latest(tmp_path)
    assert 'before_test_crash' in text and 'test-part' in text
    assert 'Fatal Python error' in text and 'native_crash_probe' in text
    assert 'process_exit' not in text
    assert json.loads((Path(info['folder']) / 'session.json').read_text())['state'] == 'started'


def test_start_is_idempotent_and_sessions_do_not_overwrite(tmp_path):
    code = '''from robocad.diagnostics import session_info
first=session_info()
assert start() == first
event('idempotent_session')
'''
    assert run_session(tmp_path, code).returncode == 0
    first, _ = latest(tmp_path)
    assert run_session(tmp_path, code).returncode == 0
    second, _ = latest(tmp_path)
    assert first['folder'] != second['folder']
    assert Path(first['log_path']).is_file()
    assert len(list(tmp_path.glob('*/session.log'))) == 2


def test_collect_matches_native_report_pid_and_start_time(tmp_path):
    session = tmp_path / 'sessions/one'
    reports = tmp_path / 'reports'
    session.mkdir(parents=True); reports.mkdir()
    now = datetime.now(timezone.utc).isoformat()
    (session / 'session.json').write_text(json.dumps({'pid': 123, 'started_at': now}))
    for name, pid, launched in [('ours', 123, now), ('other-app', 456, now),
                                ('reused-pid', 123, '2020-01-01T00:00:00+00:00')]:
        (reports / (name + '.ips')).write_text('{}\n' + json.dumps({'pid': pid, 'procLaunch': launched}))
    (reports / 'incomplete.ips').write_text('{')
    copied = collect_native_reports(tmp_path / 'sessions', reports)
    assert copied == [str(session / 'ours.ips')]
    assert (reports / 'ours.ips').read_bytes() == (session / 'ours.ips').read_bytes()
    assert collect_native_reports(tmp_path / 'sessions', reports) == []
