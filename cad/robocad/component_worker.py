"""Private child-process entry point for component geometry and display work."""
import json
import os
from pathlib import Path
import pickle
import sys

def main():
    control = os.fdopen(os.dup(1), 'w', buffering=1)
    from .diagnostics import start, event
    session = start('component-worker')
    print(json.dumps({'diagnostics': session['log_path']}), file=control, flush=True)
    from .component_jobs import body_table, pack, unpack, OPERATIONS
    from .commands import Ops
    def progress(*value): print(json.dumps({'progress': value}), file=control, flush=True)
    with open(sys.argv[1],'rb') as stream: packed,operation,args,kwargs=pickle.load(stream)
    event('component_operation', operation=operation)
    if operation not in OPERATIONS: raise ValueError('Unsupported component operation')
    doc=unpack(packed,progress=progress)
    original=body_table(doc); original_nodes=dict(doc.nodes)
    doc._component_progress=progress
    progress('Rebuilding components',0,0,'')
    result=getattr(Ops(doc),operation)(*args,**kwargs)
    from .ui.viewport import prepare_render_item, _curve_item
    nodes=[n for n in doc.nodes.values() if n.kind in ('body','sheet','curve','instance','mesh') and
           (n.id not in original_nodes or n.body is not original_nodes[n.id].body)]
    items={}
    for index,node in enumerate(nodes):
        progress('Preparing display',index,len(nodes),node.name)
        if node.kind=='curve': item=_curve_item(doc,node)
        else:
            mesh=doc.mesh_of(node.id)
            item=prepare_render_item(doc,node,mesh) if mesh and mesh.vertices else None
        if item is not None: items[node.id]=item
    progress('Preparing display',len(nodes),len(nodes),'')
    output=pack(doc,original,progress)
    with open(sys.argv[2],'wb') as stream: pickle.dump((output,items,doc.mesh_cache,result),stream,protocol=5)
    control.close()

if __name__=='__main__': main()
