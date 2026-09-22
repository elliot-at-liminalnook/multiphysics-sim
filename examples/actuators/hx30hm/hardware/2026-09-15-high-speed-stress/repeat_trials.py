"""Finite repeats of reviewed plans, with stop and measured-condition gates."""
import json,os,subprocess
from pathlib import Path
root=Path(__file__).resolve().parent
for name,cap in [('cap350-02',350),('cap450-02',450),('cap350-03',350),('cap450-03',450)]:
 with (root/(name+'.log')).open('x') as log:
  result=subprocess.run(['target/debug/examples/characterize_hx_bridge','/dev/cu.usbserial-20250303171','2','10,11,12',str(root/name),str(root/f'plans/cap{cap}.json')],env=dict(os.environ,HX_BAUD='1000000'),stdout=log,stderr=subprocess.STDOUT)
 if result.returncode:raise SystemExit(f'{name} acquisition failed; no further motion')
 subprocess.run(['python3',str(root/'review_capture.py'),name],check=True,stdout=subprocess.DEVNULL)
 r=json.loads((root/name/'telemetry-summary.json').read_text())
 assert r['completed'] and r['stop_verified']
 sag=max(x['drop_percent'] for x in r['axes']);temp=max(x['maximum_temperature_c'] for x in r['axes'])
 print(name,'completed, stopped, sag',round(sag,2),'%, max temperature',temp,flush=True)
 if sag>=10 or temp>=52:raise SystemExit('Repeat envelope exceeded; no further motion')
