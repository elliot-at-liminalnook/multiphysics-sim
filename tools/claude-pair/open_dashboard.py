#!/usr/bin/env python3
"""Start/reuse this pair's local dashboard; optionally open it in a browser."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time
from urllib.request import urlopen
import webbrowser


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state", required=True)
    parser.add_argument("--open", action="store_true")
    args = parser.parse_args()
    root = Path(args.state).expanduser().resolve()
    expected = json.loads((root / "config.json").read_text())["worktree"]

    def existing():
        try:
            url = json.loads((root / "dashboard.json").read_text())["url"]
            if not url.startswith("http://127.0.0.1:"):
                return None
            with urlopen(url + "/api/state", timeout=2) as response:
                if json.load(response)["workspace"] == expected:
                    return url
        except (OSError, ValueError, KeyError):
            pass
        return None

    url = existing()
    if not url:
        with (root / "dashboard.log").open("ab") as log:
            process = subprocess.Popen([sys.executable, str(Path(__file__).parent / "dashboard.py"),
                "--state", str(root), "--port", "0"], stdin=subprocess.DEVNULL,
                stdout=log, stderr=log, start_new_session=True)
        deadline = time.monotonic() + 10
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
