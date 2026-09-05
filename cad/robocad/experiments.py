"""Shared asynchronous experiment service for GUI, CLI and REST clients."""
import json
import importlib.metadata
import hashlib
import io
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time
import uuid
import zipfile
from copy import deepcopy
from .snapshots import capture, canonical, digest
from .kernel import KernelError
from . import experiment_config
from . import experiment_records

ROOT = Path(__file__).resolve().parents[2]
TERMINAL = {'completed','failed','cancelled'}
DEFAULT_SYSTEM = 'let assembly = cad("assembly");\n'
DEFAULT_CONTROLLER = '''fn control(t, sensors, commands, state) {
    let p = parameters();
    let target = if t < 0.2 { 0.0 } else { p.target };
    for name in commands.keys() { commands[name] = target; }
    #{ commands: commands, state: state }
}
'''


def write_json(path, data):
    path = Path(path)
    tmp = path.with_name(path.name + '.' + uuid.uuid4().hex + '.tmp')
    tmp.write_bytes(canonical(data))
    os.replace(tmp,path)


def write_artifact(path, data, executable=False):
    path = Path(path)
    # Compare the captured bytes directly. Hashing both copies of a 10 MB
    # executable here duplicates the provenance hash and stalls the UI.
    if path.exists() and path.read_bytes() == data: return
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + '.' + uuid.uuid4().hex + '.tmp')
    tmp.write_bytes(data)
    if executable and os.name != 'nt': tmp.chmod(0o755)
    os.replace(tmp, path)


def binary_digest(data):
    # BLAKE2 avoids the slow software SHA-256 path on older desktop CPUs.
    # Tag the algorithm so historical SHA-256 records stay unambiguous.
    return 'blake2b-256-' + hashlib.blake2b(data, digest_size=32).hexdigest()


def source_files():
    archive = getattr(__loader__, 'archive', None)
    if archive:
        with zipfile.ZipFile(archive) as bundle:
            return {name[len('robocad/'):]: bundle.read(name) for name in sorted(bundle.namelist())
                    if name.startswith('robocad/') and name.endswith('.py')}
    package = Path(__file__).resolve().parent
    return {str(path.relative_to(package)): path.read_bytes() for path in sorted(package.rglob('*.py'))
            if 'ui' not in path.relative_to(package).parts}


def package_versions():
    versions = {}
    for name in ('cadquery-ocp', 'numpy', 'scipy', 'trimesh'):
        distribution = importlib.metadata.distribution(name)
        metadata = distribution.read_text('METADATA') or distribution.read_text('PKG-INFO') or ''
        # Version is a single core-metadata header. Parsing the full wheel's
        # description as an email message adds unnecessary work on each run.
        header = metadata.replace('\r\n', '\n').split('\n\n', 1)[0]
        version = next((line.partition(':')[2].strip() for line in header.splitlines()
                        if line.startswith('Version:')), None)
        if not version: raise KernelError(f'Missing installed version for {name}')
        versions[name] = version
    return versions


def sources(value, default_name, default_text):
    if value is None: value = {'entry':default_name,'files':{default_name:default_text}}
    if isinstance(value,str): value = {'entry':default_name,'files':{default_name:value}}
    if not isinstance(value,dict) or set(value) != {'entry','files'} or not isinstance(value['files'],dict):
        raise KernelError('Sources require entry and files fields')
    for name, text in value['files'].items():
        if not isinstance(name,str) or not isinstance(text,str) or '\\' in name or Path(name).is_absolute() or '..' in Path(name).parts:
            raise KernelError('Script files must have relative names and text contents')
    if value['entry'] not in value['files']: raise KernelError('Entry script is missing from captured sources')
    return deepcopy(value)


class RevisionConflict(KernelError):
    pass


class Experiments:
    def __init__(self, doc=None, root=None, binary=None):
        self.doc = doc
        self.root = Path(root or ROOT/'runs'/'experiments')
        self.binary = str(binary or ROOT/'target'/'release'/('sim-experiment.exe' if os.name=='nt' else 'sim-experiment'))
        self.lock = threading.RLock()
        self.jobs = {}
        self.processes = {}
        self.owned = set()
        self.leases = {}
        self.cancelled = set()
        self.changed = []
        self._gate = threading.Semaphore(1)

    def _capture_runner(self):
        binary = Path(self.binary)
        if not binary.is_file(): raise KernelError('Build the experiment runner: cargo build --release --bin sim-experiment')
        executable = binary.read_bytes()
        binary_hash = binary_digest(executable)
        captured_binary = self.root/'artifacts'/'binaries'/binary_hash/binary.name
        write_artifact(captured_binary, executable, executable=True)
        files = source_files()
        identity = {'sources': {name: digest(data) for name, data in files.items()},
                    'packages': package_versions()}
        source_hash = digest(canonical(identity))
        captured_source = self.root/'artifacts'/'python'/(source_hash+'.zip')
        # One atomic, deterministic archive avoids dozens of file writes in the
        # UI thread. Python imports the captured package directly from this ZIP.
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, 'w', compression=zipfile.ZIP_STORED) as bundle:
            for name, data in files.items(): bundle.writestr(zipfile.ZipInfo('robocad/'+name), data)
        write_artifact(captured_source, buffer.getvalue())
        return {'binary': str(captured_binary.resolve()), 'python': str(captured_source.resolve()),
                'provenance': {'binary_hash': binary_hash, 'derivation': identity,
                               'python_version': sys.version, 'python_executable': sys.executable}}

    def list(self):
        with self.lock:
            if self.root.exists():
                for path in self.root.glob('*/run.json'):
                    if path.parent.name in self.owned: continue
                    try:
                        record=experiment_records.observe(path.parent)
                        self.jobs[path.parent.name]=record
                    except (OSError,ValueError): continue
            return [deepcopy(j) for j in sorted(self.jobs.values(),key=lambda j:j['created_at'],reverse=True)
                if self.doc is None or j.get('document_id')==self.doc.document_id]

    def catalogue(self):
        if not Path(self.binary).is_file(): raise KernelError('Build the experiment runner to discover registered components')
        process = subprocess.run([self.binary, 'catalogue'], capture_output=True, text=True, timeout=10)
        if process.returncode: raise KernelError(process.stderr.strip() or 'Component discovery failed')
        return json.loads(process.stdout)

    def get(self,run_id):
        with self.lock:
            if run_id not in self.jobs: self.list()
            if run_id not in self.jobs: raise KeyError(run_id)
            if run_id not in self.owned:
                self.jobs[run_id] = experiment_records.observe(self.root/run_id)
            return deepcopy(self.jobs[run_id])

    def _update(self,run_id,**fields):
        with self.lock:
            if run_id in self.cancelled and fields.get('state') not in ('cancelling','cancelled'): return
            self.jobs[run_id] = experiment_records.update(self.root/run_id, **fields)
        for callback in list(self.changed):
            try: callback(run_id)
            except Exception:
                import logging
                logging.exception('Experiment observer failed for %s', run_id)

    def create(self,request,document=None):
        request=experiment_config.request(request)
        expected=request.pop('expected_revision',None)
        doc = self.doc if document is None else document
        with self.lock:
            if doc is not None:
                with doc._lock:
                    if type(expected) is not int or expected!=doc.revision:
                        raise RevisionConflict(f'Expected document revision {expected}; current revision is {doc.revision}')
                    snapshot=capture(doc)
                    from .component_graph import validate_graph
                    graph = validate_graph(doc.component_graph, doc)
            else:
                snapshot=None
                from .component_graph import empty_graph
                graph = empty_graph()
            system=sources(request.get('system'),'system.rhai',DEFAULT_SYSTEM if snapshot else '')
            controller=request.get('controller',{'language':'rhai','parameters':{'target':.3}} if snapshot else None)
            if controller and controller.get('language')=='rhai':
                controller['sources']=sources(controller.get('sources'),'controller.rhai',DEFAULT_CONTROLLER)
            settings=request['settings']
            runner = self._capture_runner()
            run_id=uuid.uuid4().hex
            folder=self.root/run_id;folder.mkdir(parents=True)
            provenance={'document_id':snapshot.document_id if snapshot else None,'revision':snapshot.revision if snapshot else None,
                'physical_hash':snapshot.physical_hash if snapshot else None,'cad_archive_hash':snapshot.archive_hash if snapshot else None,
                'cad_derivation_hash': snapshot.cad_derivation_hash if snapshot else None,
                'source_hash':digest(canonical(system)),'controller_hash':digest(canonical(controller)),
                'parameters_hash':digest(canonical(request.get('parameters',{}))), 'candidate_id':request.get('candidate_id'),
                'seed':request['seed'], 'component_graph_hash': digest(canonical(graph))}
            provenance.update(runner['provenance'])
            spec={'version':1,'run_id':run_id,'system':system,'parameters':request.get('parameters',{}),
                'preflight':request['preflight'],
                'controller':controller,'settings':settings,'provenance':provenance,'cad':{},
                'seed':request['seed'],'profile':request['profile'], 'component_graph':graph}
            write_json(folder/'input.json',spec)
            if snapshot: (folder/'model.rcad').write_bytes(snapshot.data)
            record={'id':run_id,'created_at':time.time(),'updated_at':time.time(),'state':'queued','fraction':0,
                'document_id':provenance['document_id'],'revision':provenance['revision'],
                'label':request.get('label',f'Experiment {run_id[:8]}'),'provenance':provenance,
                'directory':str(folder.resolve()),'settings':settings,'parent_run':request.get('parent_run'),
                'profile':request['profile'],'seed':request['seed'],
                'preflight':request['preflight'],
                'runner': {'binary': runner['binary'], 'python': runner['python']}}
            self.leases[run_id] = experiment_records.Lease.acquire(folder/'owner.lock')
            self.owned.add(run_id)
            self.jobs[run_id]=record;write_json(folder/'run.json',record)
            thread=threading.Thread(target=self._run,args=(run_id,),daemon=True)
            thread.start()
            return deepcopy(record)

    def _run(self,run_id):
        with self._gate:
            if run_id in self.cancelled:return
            folder=self.root/run_id
            try:
                env=dict(os.environ)
                runner = self.jobs[run_id]['runner']
                env['PYTHONPATH']=runner['python']+os.pathsep+env.get('PYTHONPATH','')
                command=[sys.executable,'-m','robocad.experiment_worker',str(folder.resolve()),runner['binary']]
                kwargs={'start_new_session':True} if os.name!='nt' else {'creationflags':subprocess.CREATE_NEW_PROCESS_GROUP}
                if os.name != 'nt': kwargs['pass_fds'] = (self.leases[run_id].file.fileno(),)
                with (folder/'stderr.log').open('w') as errors:
                    with self.lock:
                        if run_id in self.cancelled:return
                        process=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=errors,text=True,env=env,**kwargs)
                        self.processes[run_id]=process
                        self._update(run_id,state='building',pid=process.pid)
                    with (folder/'events.jsonl').open('w') as events:
                        for line in process.stdout:
                            events.write(line);events.flush()
                            try:
                                event=json.loads(line)
                                fields={k:v for k,v in event.items() if k in ('stage','fraction','error','cache','timing')}
                                if event.get('state') in ('building','running'):fields['state']=event['state']
                                self._update(run_id,**fields)
                            except ValueError: pass
                    code=process.wait()
                if run_id in self.cancelled:
                    self._update(run_id,state='cancelled',exit_code=code)
                elif code==0 and (folder/'result.json').exists():
                    result=json.loads((folder/'result.json').read_text())
                    self._update(run_id,state='completed',fraction=1,exit_code=code,timing=result.get('timing',{}),
                                 evaluation=result.get('evaluation',{}))
                else:
                    self._update(run_id,state='failed',exit_code=code,error=self.get(run_id).get('error') or (folder/'stderr.log').read_text()[-6000:] or 'Worker exited without a result')
            except Exception as error:
                self._update(run_id,state='failed',error=f'{type(error).__name__}: {error}')
            finally:
                with self.lock:
                    self.processes.pop(run_id,None)
                    lease = self.leases.pop(run_id, None)
                    if lease: lease.close()

    def cancel(self,run_id):
        with self.lock:
            job=self.get(run_id)
            if job['state'] in TERMINAL:return job
            if run_id not in self.owned:
                raise KernelError('This run is owned by another live editor or worker. Cancel it in the originating editor; history continues to refresh here.')
            self.cancelled.add(run_id)
            process=self.processes.get(run_id)
            self._update(run_id,state='cancelling' if process else 'cancelled',stage='cancelling' if process else 'cancelled')
            if process is None:
                lease = self.leases.pop(run_id, None)
                if lease: lease.close()
            if process and process.poll() is None:
                def terminate():
                    if os.name=='nt':subprocess.run(['taskkill','/PID',str(process.pid),'/T','/F'],capture_output=True)
                    else:
                        try:os.killpg(process.pid,signal.SIGTERM)
                        except ProcessLookupError:return
                        try:process.wait(timeout=1)
                        except subprocess.TimeoutExpired:pass
                        # The worker may exit before a controller that ignores
                        # SIGTERM. Always finish cancelling the entire group.
                        try:os.killpg(process.pid,signal.SIGKILL)
                        except ProcessLookupError:pass
                threading.Thread(target=terminate,daemon=True).start()
            return self.get(run_id)

    def result(self,run_id):
        job=self.get(run_id)
        path=self.root/run_id/'result.json'
        if job['state']!='completed' or not path.exists():raise KernelError('This run has no completed result')
        result=json.loads(path.read_text())
        if self.doc is not None:
            provenance = result.get('provenance', {})
            from .component_graph import empty_graph
            graph_hash = digest(canonical(self.doc.component_graph))
            graph_stale = graph_hash != provenance.get('component_graph_hash', digest(canonical(empty_graph())))
            result['stale'] = graph_stale or (provenance.get('uses_cad', True) and provenance.get('physical_hash')!=capture(self.doc).physical_hash)
        return result

    def inputs(self, run_id):
        self.get(run_id)
        return json.loads((self.root/run_id/'input.json').read_text())

    def diagnostics(self, run_id):
        job = self.get(run_id)
        folder = self.root/run_id
        return {'run_id': run_id, 'state': job['state'], 'error': job.get('error'),
                'stderr': (folder/'stderr.log').read_text()[-20000:] if (folder/'stderr.log').exists() else '',
                'partial': job['state'] != 'completed' and ((folder/'result.json').exists() or (folder/'partial.json').exists())}

    def partial(self, run_id):
        job = self.get(run_id)
        if job['state'] not in ('failed', 'cancelled'): raise KernelError('Partial output is only available for failed or cancelled runs')
        folder = self.root/run_id
        path = folder/'partial.json'
        if not path.exists(): path = folder/'result.json'
        if not path.exists(): raise KernelError('This run retained no partial samples')
        result = json.loads(path.read_text())
        result.update(partial=True, state=job['state'], error=job.get('error'))
        for name in ('component_graph_mapping', 'component_derivations', 'script_component_mapping'):
            evidence = folder/(name+'.json')
            if evidence.exists(): result[name] = json.loads(evidence.read_text())
        result.pop('evaluation', None)
        return result

    def compare(self, baseline_id, candidate_id):
        from .experiment_results import compare
        return compare(self.result(baseline_id), self.result(candidate_id))

    def components(self, run_id):
        job = self.get(run_id)
        folder = self.root/run_id
        imported = folder/'imported_components.json'
        if not imported.exists(): raise KernelError('This check has not produced an imported component list; inspect its diagnostics')
        resolved = folder/'resolved_components.json'
        return {'run_id': run_id, 'revision': job.get('revision'), 'state': job['state'], 'error': job.get('error'),
            'stale': bool(self.doc and capture(self.doc).physical_hash != job.get('provenance', {}).get('physical_hash')),
            'imported': json.loads(imported.read_text()),
            'resolved': json.loads(resolved.read_text()) if resolved.exists() else None}

    def source_bundles(self, run_id):
        self.get(run_id)
        folder = self.root/run_id
        path = folder/'composition.json'
        if not path.exists(): path = folder/'input.json'
        captured = json.loads(path.read_text())
        return {'system': captured['system'], 'controller': (captured.get('controller') or {}).get('sources')}

    def captured_document(self, run_id):
        from .document import Document
        self.get(run_id)
        plan = self.root/run_id/'system.json'
        if plan.exists() and not json.loads(plan.read_text()).get('cad'): return None
        path = self.root/run_id/'model.rcad'
        if not path.exists(): return None
        doc = Document.load(str(path))
        doc.path = None
        return doc

    def close(self):
        # Each manager only cancels workers it owns, not other editor windows.
        with self.lock:
            for run_id, job in list(self.jobs.items()):
                if job['state'] not in TERMINAL and run_id in self.owned:
                    self.cancel(run_id)
