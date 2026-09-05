"""Export the completed robot outside the GUI; fixture is explicit per experiment."""
import argparse,json,time
from pathlib import Path
from robocad.document import Document
from robocad.physical import export_physical_model
ROOT=Path(__file__).resolve().parents[2]
def main():
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument('--cad',type=Path,default=ROOT/'runs/robot-imports/Full_Bot-knee-01.rcad')
 parser.add_argument('--out',type=Path,default=ROOT/'runs/full-robot/bench.simrobot.json')
 parser.add_argument('--free',action='store_true',help='leave chassis floating; default is a bench fixture')
 args=parser.parse_args();d=Document.load(str(args.cad));print('Loaded CAD',d.revision,flush=True)
 if not args.free:d.nodes['93c3343067fe'].robot={**(d.nodes['93c3343067fe'].robot or {}),'ground':True}
 started=time.monotonic();m=export_physical_model(d,flex=False,verbose=True)
 m['source']['cad_revision']=d.revision;m['source']['fixture']='floating' if args.free else 'chassis fixed to bench'
 m['source']['fidelity']='commissioning model; provisional material, inertia, servo dynamics and losses; ideal transmissions'
 args.out.parent.mkdir(exist_ok=True,parents=True);args.out.write_text(json.dumps(m))
 print('Exported',len(m['links']),'links',len(m['motors']),'motors',len(m['transmissions']),'transmissions in',time.monotonic()-started,'s',flush=True)
if __name__=='__main__':main()
