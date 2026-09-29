"""Print jobs shared by the REST API and the Print menu: split for printing,
check strength, plan settings, assembly guides and test coupons.

Every job works on a snapshot copy of the document off the UI thread and
publishes its outcome as one undoable step (new nodes, or results attached to
nodes). Long jobs run in a background thread with progress and cancellation;
``get`` reports their state. The source geometry is never modified.
"""

from __future__ import annotations

import io
import os
import threading
import time
import traceback
import uuid
from dataclasses import dataclass, field
from typing import Any, Callable, Optional

from . import print_registry as reg
from . import print_study as ps

RUNS = os.path.join(reg.REPO, 'runs', 'cad-print')


@dataclass
class Job:
    id: str
    kind: str
    state: str = 'queued'          # queued | running | done | failed | cancelled
    fraction: float = 0.0
    message: str = ''
    result: Any = None
    error: Optional[str] = None
    started: float = field(default_factory=time.time)
    finished: Optional[float] = None
    out_dir: Optional[str] = None
    cancel: threading.Event = field(default_factory=threading.Event)
    run: Any = None

    def public(self) -> dict:
        return {'id': self.id, 'kind': self.kind, 'state': self.state, 'fraction': round(self.fraction, 3), 'message': self.message,
                'error': self.error, 'result': self.result, 'out_dir': self.out_dir,
                'seconds': round((self.finished or time.time()) - self.started, 2)}


def region_from(doc, node_id: str, spec: dict) -> dict:
    """A study region from a REST/UI description: plain regions pass through;
    ``contact`` (another node), ``faces`` (face indices) and ``bottom`` are
    found from the geometry."""
    if not isinstance(spec, dict) or len(spec) == 0:
        raise ValueError(f'region must be an object, got {spec!r}')
    if 'contact' in spec:
        return ps.contact_region(doc, node_id, spec['contact'], gap=float(spec.get('gap', 0.3)))
    if 'faces' in spec:
        return ps.faces_region(doc, node_id, spec['faces'], radius=float(spec.get('radius', 1.0)))
    if 'bottom' in spec:
        up = spec['bottom'] if isinstance(spec['bottom'], (list, tuple)) else (0, 0, 1)
        return ps.bottom_region(doc, node_id, tuple(up), float(spec.get('depth', 0.3)))
    known = {'sphere', 'box', 'cylinder', 'points', 'below'}
    if len(spec) != 1 or next(iter(spec)) not in known:
        raise ValueError(f"region: use one of {sorted(known | {'contact', 'faces', 'bottom'})}")
    return spec


class PrintJobs:
    def __init__(self, doc_getter: Callable[[], Any], ops_getter: Callable[[], Any], run_on_main: Callable[[Callable], Any],
                 refresh: Callable[[], None] = lambda: None):
        self.doc = doc_getter
        self.ops = ops_getter
        self.run_on_main = run_on_main
        self.refresh = refresh
        self.jobs: dict[str, Job] = {}

    # ------------------------------------------------------------ plumbing
    def _snapshot(self, expected_revision=None):
        from .candidates import check_revision
        from .snapshots import capture

        def take():
            doc = self.doc()
            with doc._lock:
                if expected_revision is not None:
                    check_revision(doc, expected_revision)
                return capture(doc)
        return self.run_on_main(take)

    @staticmethod
    def _copy(snapshot):
        from .document import Document
        doc = Document.load(io.BytesIO(snapshot.data))
        doc.path = None
        return doc

    def _publish(self, before, staged, label: str) -> int:
        from .candidates import PublishState, check_revision

        def publish():
            doc = self.doc()
            with doc._lock:
                check_revision(doc, before.revision)
                self.ops().stack.push(PublishState(doc, staged, label))
            self.refresh()
            return doc.revision
        return self.run_on_main(publish)

    def _start(self, kind: str, work: Callable[[Job], Any]) -> Job:
        job = Job(uuid.uuid4().hex[:10], kind)
        self.jobs[job.id] = job

        def body():
            job.state = 'running'
            try:
                job.result = work(job)
                job.state = 'cancelled' if job.cancel.is_set() else 'done'
                job.fraction = 1.0
            except Exception as e:  # noqa: BLE001 — reported to the caller
                job.state = 'cancelled' if job.cancel.is_set() else 'failed'
                job.error = str(e) or type(e).__name__
                if job.state == 'failed' and os.environ.get('ROBOCAD_DEBUG'):
                    job.error += '\n' + traceback.format_exc()
            finally:
                job.finished = time.time()
        threading.Thread(target=body, daemon=True, name=f'print-{kind}-{job.id}').start()
        return job

    def get(self, job_id: str) -> dict:
        if job_id not in self.jobs:
            raise KeyError(f'no print job {job_id}')
        return self.jobs[job_id].public()

    def list(self) -> list[dict]:
        return [j.public() for j in self.jobs.values()]

    def cancel(self, job_id: str) -> dict:
        job = self.jobs.get(job_id)
        if job is None:
            raise KeyError(f'no print job {job_id}')
        job.cancel.set()
        if job.run is not None:
            job.run.cancel()
        return job.public()

    def wait(self, job_id: str, timeout: float = 3600) -> dict:
        end = time.time() + timeout
        while self.jobs[job_id].state in ('queued', 'running') and time.time() < end:
            time.sleep(0.05)
        return self.get(job_id)

    def _out_dir(self, kind: str, doc) -> str:
        stem = ps.slug(os.path.splitext(os.path.basename(doc.path or 'untitled'))[0] or 'untitled')
        path = os.path.join(RUNS, f'{stem}-{kind}-{time.strftime("%Y%m%d-%H%M%S")}-{uuid.uuid4().hex[:4]}')
        os.makedirs(path, exist_ok=True)
        return path

    # ---------------------------------------------------------------- split
    def split_job(self, body: dict) -> dict:
        """The split as a background job (the menu uses this; REST may too)."""
        return self._start('split', lambda job: self.split(body, job)).public()

    def split(self, body: dict, job: Optional[Job] = None) -> dict:
        """Split a body for printing (seconds): pieces are added under a new group."""
        from .commands import Ops
        from .print_split import SplitOptions, apply_split, split_for_printing
        node = body.get('node')
        if not node:
            raise ValueError('split: give `node` (the body to split)')
        options = SplitOptions(**{k: body[k] for k in ('printer', 'joint', 'screw', 'pin_diameter', 'wall', 'area_per_screw', 'max_screws', 'extra_planes') if k in body})
        reg.usable_mm(options.printer)  # a clear error for an unknown printer
        before = self._snapshot(body.get('expected_revision'))
        staged = self._copy(before)
        if node not in staged.nodes or staged.nodes[node].body is None:
            raise ValueError(f'split: node {node} is not a body')
        if job:
            job.message = f'cutting {staged.nodes[node].name} for the {options.printer}'
        result = split_for_printing(staged, node, options)
        group = apply_split(Ops(staged), result, body.get('name'))
        summary = result.summary(staged.kernel)
        if job and job.cancel.is_set():
            raise RuntimeError('cancelled before publishing')
        revision = self._publish(before, staged, f'Split {staged.nodes[node].name} for printing')
        pieces = [c for c in staged.nodes[group].children]
        return {**summary, 'revision': revision, 'group': group, 'piece_nodes': pieces}

    # -------------------------------------------------------------- analyze
    def _parts(self, doc, body: dict) -> list:
        parts = []
        for i, p in enumerate(body.get('parts') or []):
            nid = p.get('node')
            if nid not in doc.nodes or doc.nodes[nid].body is None:
                raise ValueError(f'parts[{i}].node {nid!r} is not a body')
            try:
                fixtures = [{'name': f.get('name', f'fixture {k + 1}'), 'region': region_from(doc, nid, f['region'])} for k, f in enumerate(p.get('fixtures') or [])]
                loads = [{'name': l.get('name', f'load {k + 1}'), 'region': region_from(doc, nid, l['region']), 'direction': list(l['direction']), 'magnitude': l['magnitude']}
                         for k, l in enumerate(p.get('loads') or [])]
            except KeyError as e:
                raise ValueError(f'parts[{i}]: missing {e}') from None
            spec = ps.PartSpec(nid, p.get('name'), tuple(p.get('build_direction', (0, 0, 1))), p.get('settings') or ps.PartSpec(nid).settings,
                               fixtures, loads, tuple(p['acceleration']) if p.get('acceleration') else (0.0, 0.0, -9.81),
                               p.get('sections') or [], p.get('seams') or [], p.get('directions'))
            parts.append(spec)
        if not parts:
            raise ValueError('give `parts`: [{node, fixtures, loads, …}]')
        return parts

    def _run_tool(self, job: Job, command: str, study: str, extra=()) -> dict:
        def progress(f, m):
            job.fraction, job.message = 0.1 + 0.85 * f, m
        run = ps.Run(command, study, on_progress=progress, extra=extra)
        job.run = run
        run.start()
        while not run.done.wait(0.1):
            if job.cancel.is_set():
                run.cancel()
        return run.wait(1)

    def analyze(self, body: dict) -> dict:
        """Check strength as printed (a background job)."""
        before = self._snapshot(body.get('expected_revision'))

        def work(job: Job):
            job.message = 'finding where parts are held and loaded'
            staged = self._copy(before)
            out = self._out_dir('strength', staged)
            job.out_dir = out
            parts = self._parts(staged, body)
            study = ps.write_study(staged, parts, out, printer=body.get('printer', 'bambu-h2c'), material=body.get('material', 'pla-basic'),
                                   simulation=body.get('simulation'), safety_target=float(body.get('safety_target', 2.0)),
                                   voxels=int(body.get('voxels', 40000)), voxel_mm=body.get('voxel_mm'),
                                   provenance={'cad_revision': before.revision})
            result = self._run_tool(job, 'analyze', study)
            res_dir = os.path.join(out, 'print-results')
            # Attach each part's result to its node (the viewport colours from it).
            for spec, part in zip(parts, result['parts']):
                staged.nodes[spec.node_id].results = {
                    'section': 'print', 'result_dir': res_dir, 'field': part['field'], 'safety_factor': part['safety_factor'],
                    'governing': part['governing'], 'passes': part['passes'], 'settings': part['settings'],
                    'build_direction': part['build_direction'], 'registry_sha256': result['registry_sha256'],
                    'fidelity': result['fidelity'], 'cad_revision': before.revision}
            job.message = 'publishing'
            revision = self._publish(before, staged, 'Strength check')
            return {'revision': revision, 'result': os.path.join(res_dir, 'result.json'),
                    'parts': [{'node': s.node_id, 'name': p['name'], 'safety_factor': p['safety_factor'], 'mode': p['governing']['mode'],
                               'at': p['governing']['at'], 'passes': p['passes'], 'mass_g': p['mass_g'], 'seams': [{'name': x['name'], 'safety_factor': x['safety_factor']} for x in p['seams']]}
                              for s, p in zip(parts, result['parts'])]}
        return self._start('analyze', work).public()

    # ----------------------------------------------------------------- plan
    def plan(self, body: dict) -> dict:
        """Choose orientation and settings per part, then lay out plates (a background job)."""
        before = self._snapshot(body.get('expected_revision'))

        def work(job: Job):
            from .print_plan import write_plates
            from .commands import SetAttributes
            job.message = 'finding where parts are held and loaded'
            staged = self._copy(before)
            out = self._out_dir('plan', staged)
            job.out_dir = out
            parts = self._parts(staged, body)
            printer, material = body.get('printer', 'bambu-h2c'), body.get('material', 'pla-basic')
            study = ps.write_study(staged, parts, out, printer=printer, material=material, simulation=body.get('simulation'),
                                   safety_target=float(body.get('safety_target', 2.0)), voxels=int(body.get('voxels', 40000)),
                                   voxel_mm=body.get('voxel_mm'), plan=body.get('space'), provenance={'cad_revision': before.revision})
            plan = self._run_tool(job, 'plan', study)
            plan_dir = os.path.join(out, 'print-plan')
            job.message = 'laying out plates'
            pieces = []
            for spec, part in zip(parts, plan['parts']):
                c = part.get('chosen')
                if not c:
                    raise ValueError(f"{part['name']}: no orientation fits the {printer}; split it first")
                v = part.get('verified') or {}
                pieces.append({'name': part['name'], 'body': staged.resolved_body(spec.node_id), 'build_direction': c['build_direction'],
                               'settings': c['settings'], 'estimate': c['estimate'], 'safety_factor': v.get('safety_factor'), 'source': spec.node_id})
                node = staged.nodes[spec.node_id]
                robot = dict(node.robot or {})
                robot['print_plan'] = {'build_direction': c['build_direction'], 'settings': c['settings'], 'estimate': c['estimate'],
                                       'safety_factor': v.get('safety_factor'), 'plan': os.path.join(plan_dir, 'plan.json'), 'registry_sha256': plan['registry_sha256']}
                node.robot = robot
                if v.get('field'):
                    node.results = {'section': 'print', 'result_dir': plan_dir, 'field': v['field'], 'safety_factor': v['safety_factor'],
                                    'governing': v['governing'], 'passes': v['safety_factor'] >= plan['safety_target'], 'settings': c['settings'],
                                    'build_direction': c['build_direction'], 'registry_sha256': plan['registry_sha256'], 'cad_revision': before.revision}
            manifest = write_plates(staged.kernel, pieces, os.path.join(out, 'plates'), printer, material, os.path.join(plan_dir, 'plan.json'))
            job.message = 'publishing'
            revision = self._publish(before, staged, 'Print plan')
            return {'revision': revision, 'plan': os.path.join(plan_dir, 'plan.json'), 'plates': os.path.join(out, 'plates'),
                    'total_hours': manifest['total_hours'], 'total_filament_g': manifest['total_filament_g'],
                    'parts': [{'node': s.node_id, 'name': p['name'], 'chosen': p.get('chosen'), 'verified_safety_factor': (p.get('verified') or {}).get('safety_factor'), 'notes': p['notes']}
                              for s, p in zip(parts, plan['parts'])],
                    'plate_files': [f['file'] for f in manifest['plates']]}
        return self._start('plan', work).public()

    # ------------------------------------------------------ split for strength
    def strength_split(self, body: dict) -> dict:
        """Whole or split at a junction, each piece in its strong orientation (a background job)."""
        before = self._snapshot(body.get('expected_revision'))

        def work(job: Job):
            from .print_strength_split import compare
            staged = self._copy(before)
            node = body.get('node')
            if node not in staged.nodes or staged.nodes[node].body is None:
                raise ValueError(f'strength_split: node {node!r} is not a body')
            out = self._out_dir('strength-split', staged)
            job.out_dir = out

            def run(cmd, study):
                return self._run_tool(job, cmd, study)
            verdict = compare(staged, node, body['part'], out, body.get('printer', 'bambu-h2c'), body.get('material', 'pla-basic'),
                              body.get('simulation'), float(body.get('safety_target', 2.0)), body.get('space'), body.get('planes'),
                              int(body.get('voxels', 30000)), run, lambda f, m: setattr(job, 'message', m))
            return {'recommendation': verdict['recommendation'], 'why': verdict['why'], 'plane': verdict.get('plane'),
                    'report': os.path.join(out, 'strength-split.json')}
        return self._start('strength_split', work).public()

    # ------------------------------------------------------------- assembly
    def assembly(self, body: dict) -> dict:
        """Assembly steps, hardware, an HTML guide and (optionally) an exploded view for a split (a background job)."""
        before = self._snapshot(body.get('expected_revision'))

        def work(job: Job):
            from .commands import Ops
            from .print_assembly import add_exploded_view, plan_assembly, write_guide
            staged = self._copy(before)
            group = body.get('group')
            if group not in staged.nodes:
                raise ValueError(f'assembly: {group!r} is not a split group (the group Split for printing made)')
            job.message = 'ordering the pieces'
            plan = plan_assembly(staged, group)
            out = self._out_dir('assembly', staged)
            job.out_dir = out
            job.message = 'drawing the steps'
            guide = write_guide(staged, plan, out, images=body.get('images', True))
            result = {'guide': guide, 'steps': plan['steps'], 'hardware': plan['hardware'], 'tools': plan['tools']}
            if body.get('exploded', True):
                result['exploded'] = add_exploded_view(Ops(staged), plan)
                result['revision'] = self._publish(before, staged, 'Exploded view')
            return result
        return self._start('assembly', work).public()

    # -------------------------------------------------------------- coupons
    def coupons(self, body: dict) -> dict:
        """Test coupons (material bars and copies of a split's joints), plates, a results template and a protocol."""
        before = self._snapshot(body.get('expected_revision'))

        def work(job: Job):
            from .print_coupons import write_coupon_kit
            staged = self._copy(before)
            split = None
            if body.get('group'):
                g = staged.nodes.get(body['group'])
                split = (g.robot or {}).get('print_split') if g else None
                if split is None:
                    raise ValueError(f"coupons: {body['group']!r} is not a split group")
            out = self._out_dir('coupons', staged)
            job.out_dir = out
            job.message = 'making coupons'
            return write_coupon_kit(staged.kernel, out, body.get('material', 'pla-basic'), body.get('printer', 'bambu-h2c'), split,
                                    body.get('settings'), body.get('copies'))
        return self._start('coupons', work).public()
