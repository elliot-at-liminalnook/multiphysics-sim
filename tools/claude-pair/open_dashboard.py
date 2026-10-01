#!/usr/bin/env python3
"""Start/reuse this pair's local dashboard; optionally open it in a browser."""
import argparse
import json
import socket
from pathlib import Path
import subprocess
import sys
import time
from urllib.request import urlopen
import webbrowser


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state", help="Run state directory (default: <repo>/.claude-pair)")
    parser.add_argument("--open", action="store_true")
    args = parser.parse_args()
    sys.path.insert(0, str(Path(__file__).parent))
    import pair
    root = Path(args.state).expanduser().resolve() if args.state else pair.default_state()
    expected = json.loads((root / "config.json").read_text())["worktree"]

    def existing():
        try:
            url = json.loads((root / "dashboard.json").read_text())["url"]
            if not url.startswith("http://127.0.0.1:"):
                return None
            with urlopen(url + "/api/ping", timeout=5) as response:
                if json.load(response)["workspace"] == expected:
                    return url
        except (OSError, ValueError, KeyError):
            pass
        return None

    url = existing()
    if not url:
        # Reuse the last address when it's free, so an open browser tab keeps working.
        port = "0"
        try:
            last = int(json.loads((root / "dashboard.json").read_text())["url"].rsplit(":", 1)[1])
            with socket.socket() as probe:
                probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)  # as the server does
                probe.bind(("127.0.0.1", last))
            port = str(last)
        except (OSError, ValueError, KeyError, IndexError):
            pass
        with (root / "dashboard.log").open("ab") as log:
            process = subprocess.Popen([sys.executable, str(Path(__file__).parent / "dashboard.py"),
                "--state", str(root), "--port", port], stdin=subprocess.DEVNULL,
                stdout=log, stderr=log, start_new_session=True)
        deadline = time.monotonic() + 30
        while not url and process.poll() is None and time.monotonic() < deadline:
            time.sleep(.15)
            url = existing()
        if not url:
            raise SystemExit(f"Dashboard did not start. See {root / 'dashboard.log'}")
    print(url)
    if args.open:
        webbrowser.open(url)


if __name__ == "__main__":
    main()
