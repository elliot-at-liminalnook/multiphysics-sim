#!/usr/bin/env python3
"""Serve the physics simulator planning page locally."""

from __future__ import annotations

import argparse
import os
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


PROJECT_DIR = Path(__file__).resolve().parent


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--port",
        type=int,
        default=int(os.environ.get("PORT", "8000")),
        help="port to listen on (default: PORT or 8000)",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    handler = partial(SimpleHTTPRequestHandler, directory=PROJECT_DIR)
    server = ThreadingHTTPServer(("127.0.0.1", args.port), handler)

    print(f"Serving {PROJECT_DIR / 'index.html'} at http://127.0.0.1:{args.port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nServer stopped.")
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
