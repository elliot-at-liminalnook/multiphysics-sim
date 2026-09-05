# Windows: create the environment on first run, then launch robocad.
Set-Location $PSScriptRoot
if (-not (Test-Path ".venv\Scripts\python.exe")) {
  py -3.9 -m venv .venv
  .venv\Scripts\pip install --upgrade pip
  .venv\Scripts\pip install -r requirements.txt
}
& .venv\Scripts\python -m robocad.ui.app @args
