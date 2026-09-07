"""Export the completed robot outside the GUI; fixture is explicit per experiment."""
import argparse,hashlib,json,time
from pathlib import Path
from robocad.document import Document
from robocad.physical import export_physical_model
from robocad.derivation_cache import DerivationCache
from robocad.experiment_worker import derivation_identity
ROOT=Path(__file__).resolve().parents[2]
def main():
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument('--cad',type=Path,default=ROOT/'examples/full-robot/baseline/robot.rcad')
 parser.add_argument('--out',type=Path)
 parser.add_argument('--scene',type=Path,help='also package a shared Rust/Rhai commissioning session')
 parser.add_argument('--cache',type=Path,default=ROOT/'runs/full-robot/derived')
 parser.add_argument('--free',action='store_true',help='leave chassis floating; default is a bench fixture')
 args=parser.parse_args();d=Document.load(str(args.cad));print('Loaded CAD',d.revision,flush=True)
 args.out=args.out or ROOT/('runs/full-robot/floating.simrobot.json' if args.free else 'runs/full-robot/bench.simrobot.json')
 if not args.free:d.nodes['93c3343067fe'].robot={**(d.nodes['93c3343067fe'].robot or {}),'ground':True}
 identity=derivation_identity();cache=DerivationCache(args.cache,identity)
 started=time.monotonic();m=export_physical_model(d,flex=False,verbose=True,cache=cache)
 m['source']['cad_revision']=d.revision;m['source']['fixture']='floating' if args.free else 'chassis fixed to bench'
 m['source']['cad_sha256']=hashlib.sha256(args.cad.read_bytes()).hexdigest()
 m['source']['derivation']=identity
 m['source']['fidelity']='commissioning model; provisional material, inertia, servo dynamics and losses; ideal transmissions'
 args.out.parent.mkdir(exist_ok=True,parents=True);args.out.write_text(json.dumps(m))
 args.out.with_suffix('.derivation.json').write_text(json.dumps(cache.stats,indent=2))
 if args.scene:
  controller=(ROOT/'examples/full-robot/controller.rhai').read_text()
  scene={'version':1,'robot':m,'options':{'flex':False,'contact':args.free,'step':0.0005,'sample':0.001},
         'period_s':0.002,'duration_s':1.0,
         'controller':{'sources':{'entry':'controller.rhai','files':{'controller.rhai':controller}},'parameters':{},'inputs':[]}}
  args.scene.parent.mkdir(exist_ok=True,parents=True);args.scene.write_text(json.dumps(scene))
 print('Exported',len(m['links']),'links',len(m['motors']),'motors',len(m['transmissions']),'transmissions in',time.monotonic()-started,'s',flush=True)
if __name__=='__main__':main()
