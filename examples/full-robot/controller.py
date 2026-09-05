"""Conservative bench supervisor for all twelve actuators (simulation only)."""
import argparse,json,math,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'clients/python'))
from simloop import Loop
LEGS=('+X','-Y','+Y','-X')
ROLES=('Hip servo output','Worm servo output','Foot servo output')
CHANNELS=[f'{leg} | {role}.target' for leg in LEGS for role in ROLES]
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--exercise',action='store_true');p.add_argument('--log',required=True);args=p.parse_args()
 with Loop.stdio() as loop,open(args.log,'w') as log:
  actual={a.name for a in loop.contract.actuators}
  if actual!=set(CHANNELS):raise ValueError(f'Expected the completed twelve-actuator assembly, got {actual}')
  for frame in loop:
   # Smooth, small input-shaft motion; worm output angle is this target / 5.
   wave=(1-math.cos(math.pi*min(max(frame.t-.1,0),1)))/2 if args.exercise else 0.
   commands={name:(.02 if 'Hip ' in name else .10 if 'Worm ' in name else .04)*wave for name in CHANNELS}
   loop.send(**commands)
   log.write(json.dumps({'seq':frame.seq,'t':frame.t,'commands':commands},allow_nan=False)+'\n');log.flush()
if __name__=='__main__':main()
