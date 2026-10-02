"""Isolated preparation for component commands; commit remains revision guarded.

The private pickle transport is only exchanged with our own child process in a
new temporary directory. User library/model files remain validated ZIP/JSON.
"""
from copy import copy, deepcopy
from pathlib import Path
import json
import os
import pickle
import queue
import subprocess
import sys
import tempfile
import threading
import uuid

from .components import clone_node, ComponentChange
from .document import Document
from .kernel import KernelError

OPERATIONS = {'create_component_family', 'link_component_family','create_component', 'make_component', 'new_parametric_component',
              'place_component', 'set_component_parameters', 'set_component_overrides',
              'detach_component', 'import_component', 'export_component', 'transform_components'}


def snapshot(doc):
    """Copy metadata on the owner thread. Kernel bodies are immutable handles."""
    result = Document(doc.kernel)
    for key, value in vars(doc).items():
        if key.startswith('_') or key in ('kernel', 'listeners', 'mesh_cache', 'nodes', 'component_definitions'): continue
        setattr(result, key, deepcopy(value))
    result.nodes = {key: clone_node(n) for key,n in doc.nodes.items()}
    result.component_definitions = {}
    for key,d in doc.component_definitions.items():
        definition = copy(d)
        definition.nodes = {nid: clone_node(n) for nid,n in d.nodes.items()}
        for field in ('roots', 'materials', 'parameters', 'features', 'ports', 'provenance', 'variants'):
            setattr(definition, field, deepcopy(getattr(d,field)))
        definition.source_bytes = dict(d.source_bytes)
        result.component_definitions[key] = definition
    result._snapshot_body_cache = dict(doc._snapshot_body_cache)
    return result


def body_table(doc):
    result = {('node',nid): n.body for nid,n in doc.nodes.items() if n.body is not None}
    for key,d in doc.component_definitions.items():
        result.update({('definition',key,nid): n.body for nid,n in d.nodes.items() if n.body is not None})
    return result


def pack(doc, external=None, progress=lambda *args: None):
    refs = {id(body): key for key,body in (external or {}).items()}
    payloads = {}; cache = {id(body): data for body,data in doc._snapshot_body_cache.values()}
    for d in doc.component_definitions.values(): cache.update({id(body): data for body,data in d.source_bytes.values()})
    count = 0; total = len(doc.nodes) + sum(len(d.nodes) for d in doc.component_definitions.values())
    def node(n):
        nonlocal count
        result = clone_node(n)
        if result.image: result.image = {key: value for key,value in result.image.items() if not key.startswith('_')}
        if n.body is not None:
            ident = id(n.body)
            if ident not in refs:
                key = ('payload', len(payloads)); refs[ident] = key
                data = cache.get(ident)
                if data is None: data = doc.kernel.serialize(n.body)
                payloads[key] = (n.body.kind, data)
            result.body = refs[ident]
        count += 1; progress('Transferring geometry', count, total, n.name)
        return result
    attrs = {key: value for key,value in vars(doc).items() if not key.startswith('_') and key not in ('kernel','listeners','mesh_cache','nodes','component_definitions')}
    definitions = {}
    nodes = {nid: node(n) for nid,n in doc.nodes.items()}
    for key,d in doc.component_definitions.items():
        definition = copy(d); definition.nodes = {nid: node(n) for nid,n in d.nodes.items()}; definition.source_bytes = {}
        definitions[key] = definition
    return attrs, nodes, definitions, payloads


def unpack(packed, external=None, progress=lambda *args: None):
    attrs,nodes,definitions,payloads = packed
    doc = Document(); refs = dict(external or {})
    for index,(key,(kind,data)) in enumerate(payloads.items()):
        refs[key] = doc.kernel.deserialize(data, kind)
        progress('Receiving geometry', index+1, len(payloads), '')
    def restore(n, cache):
        if n.body is not None:
            key = n.body; n.body = refs[key]
            if key in payloads: cache[n.id] = (n.body, payloads[key][1])
    for n in nodes.values(): restore(n, doc._snapshot_body_cache)
    for d in definitions.values():
        for n in d.nodes.values(): restore(n, d.source_bytes)
    for key,value in attrs.items(): setattr(doc,key,value)
    doc.nodes,doc.component_definitions = nodes,definitions
    return doc


class ComponentJob:
    def __init__(self, doc, operation, args=(), kwargs=None):
        if operation not in OPERATIONS: raise KernelError('Unsupported background component command')
        self.id = uuid.uuid4().hex
        self.document_id, self.revision = doc.document_id, doc.revision
        self.input = snapshot(doc)
        self.operation,self.args,self.kwargs = operation,args,kwargs or {}
        self.messages = queue.Queue(); self.stop = threading.Event(); self.process = None
        self.state = 'pending'; self.stage = ''; self.done = self.total = 0; self.error = None
        self.result = None; self.prepared = None; self.output = None
        self.log_path = None
        self.prepare_display = False
        self.export_temporary = None
        self.export_target = None

    def start(self):
        self.state = 'running'
        threading.Thread(target=self.run, daemon=True, name='component-transfer').start()
        return self.id

    def cancel(self):
        if self.state in ('applied','failed','cancelled'): return
        self.stop.set()
        self.terminate_worker()

    def terminate_worker(self):
        process = self.process
        if process is not None and process.poll() is None:
            try: process.terminate()
            except ProcessLookupError: pass  # Child exited between poll and signal.

    def progress(self, stage, done, total, name=''):
        if self.stop.is_set(): raise InterruptedError('Component preparation cancelled')
        self.messages.put(('progress',(stage,done,total,name)))

    def run(self):
        try:
            with tempfile.TemporaryDirectory(prefix='robocad-component-') as folder:
                root=Path(folder); request=root/'input'; response=root/'output'
                args, kwargs = list(self.args), dict(self.kwargs)
                if self.operation == 'export_component':
                    target = kwargs.get('path') if 'path' in kwargs else args[1]
                    self.export_target = str(Path(target).expanduser().resolve())
                    # Same directory permits an atomic rename after the owner guard.
                    handle = tempfile.NamedTemporaryFile(prefix='.robocad-component-', suffix='.rcomp',
                        dir=Path(self.export_target).parent, delete=False)
                    self.export_temporary = handle.name
                    handle.close()
                    if 'path' in kwargs: kwargs['path'] = self.export_temporary
                    else: args[1] = self.export_temporary
                packed = pack(self.input, progress=self.progress)
                with request.open('wb') as stream: pickle.dump((packed,self.operation,args,kwargs,self.prepare_display), stream, protocol=5)
                self.progress('Starting rebuild',0,0)
                env=os.environ.copy(); env['PYTHONPATH']=str(Path(__file__).resolve().parents[1])+os.pathsep+env.get('PYTHONPATH','')
                with (root/'errors').open('w+') as errors:
                    self.process=subprocess.Popen([sys.executable,'-m','robocad.component_worker',str(request),str(response)],
                        stdout=subprocess.PIPE,stderr=errors,text=True,env=env)
                    if self.stop.is_set(): self.terminate_worker()
                    for line in self.process.stdout:
                        message=json.loads(line)
                        if 'progress' in message: self.progress(*message['progress'])
                        if 'diagnostics' in message: self.log_path = message['diagnostics']
                    self.process.stdout.close()
                    code=self.process.wait()
                    if self.stop.is_set(): raise InterruptedError()
                    if code:
                        errors.seek(0)
                        detail = errors.read()[-3000:]
                        if self.log_path:
                            detail = Path(self.log_path).read_text(errors='replace')[-3000:]
                            detail += '\nDiagnostics: ' + self.log_path
                        raise KernelError(detail or 'Component worker failed')
                with response.open('rb') as stream: packed,items,meshes,result=pickle.load(stream)
                if self.export_target is not None: result['path'] = self.export_target
                candidate=unpack(packed,body_table(self.input),self.progress)
                candidate.mesh_cache=meshes
                self.progress('Ready to apply',1,1)
                self.messages.put(('ready',(candidate,items,result)))
        except InterruptedError: self.messages.put(('cancelled',None))
        except Exception as error: self.messages.put(('failed',str(error)))
        finally:
            if self.process and self.process.poll() is None:
                self.terminate_worker(); self.process.wait()

    def poll(self):
        # Bounded drain: progress is coalesced by callers' regular timer.
        for _ in range(256):
            try: kind,value=self.messages.get_nowait()
            except queue.Empty: break
            if kind=='progress': self.stage,self.done,self.total,_=value
            elif kind=='ready':
                if self.stop.is_set(): self.state='cancelled'; self.release()
                else: self.output,self.prepared,self.result=value; self.state='ready'
            elif kind=='failed': self.error=value; self.state='failed'; self.release()
            elif kind=='cancelled': self.state='cancelled'; self.release()
        return self.status()

    def status(self):
        return {'id':self.id,'operation':self.operation,'state':self.state,'stage':self.stage,'done':self.done,'total':self.total,
                'document_id':self.document_id,'revision':self.revision,
                'error':self.error,'log_path':self.log_path,'result':self.result if self.state=='applied' else None}

    def commit(self, ops, display=None):
        doc=ops.doc
        if self.state!='ready': raise KernelError('Component result is not ready')
        if self.stop.is_set(): self.state='cancelled'; self.release(); return
        if doc.document_id!=self.document_id or doc.revision!=self.revision:
            self.state='failed'; self.error='The document changed during preparation. Your edits are preserved; retry the component operation.'
            self.release()
            return
        candidate=self.output
        if self.operation!='export_component':
            # Keep unchanged meshes; new meshes were prepared in the child process.
            meshes={key: mesh for key,mesh in doc.mesh_cache.items() if key[0] in candidate.nodes and candidate.nodes[key[0]].body is doc.nodes.get(key[0],candidate.nodes[key[0]]).body}
            meshes.update(candidate.mesh_cache)
            command=ComponentChange(doc, self.operation.replace('_',' ').title(),candidate.component_definitions,candidate.nodes,candidate.roots,candidate.materials)
            command.prepared_meshes=meshes
            if display is not None:
                command.before_display=dict(display)
                command.after_display={nid:item for nid,item in display.items() if nid in candidate.nodes}
                command.after_display.update(self.prepared)
            ops.stack.push(command)
            doc._snapshot_body_cache.update(candidate._snapshot_body_cache)
        if self.operation=='export_component':
            os.replace(self.export_temporary, self.export_target)
            self.export_temporary = None
        self.state='applied'
        # Release the transfer snapshot; the undo command owns the necessary state.
        self.input=None; self.output=None

    def release(self):
        if self.export_temporary is not None:
            Path(self.export_temporary).unlink(missing_ok=True)
            self.export_temporary = None
        self.input = None
        self.output = None
        self.prepared = None

    def fail(self, error):
        self.state = 'failed'
        self.error = error
        self.release()
