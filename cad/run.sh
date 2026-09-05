#!/usr/bin/env bash
# macOS / Linux: create the environment on first run, then launch robocad.
set -e
cd "$(dirname "$0")"
if [ ! -x .venv/bin/python ]; then
  python3 -m venv .venv
  .venv/bin/pip install --upgrade pip
  .venv/bin/pip install -r requirements.txt
fi
exec .venv/bin/python -m robocad.ui.app "$@"
