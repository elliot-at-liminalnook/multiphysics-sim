"""One owner for component preparation and revision-guarded undo publication.

Document/undo/display access is owner-thread work. Geometry and file preparation
belongs to ComponentJob's isolated worker. Qt owns only widgets and selection;
REST polling and the Qt timer both advance this same owner under the document
lock. Jobs retain their captured identity across document replacement.
"""
from .component_jobs import ComponentJob
from .kernel import KernelError

ACTIVE = ('pending', 'running', 'ready')


class ComponentJobService:
    def __init__(self, get_ops, get_display=lambda: None, refresh=lambda: None, job_factory=ComponentJob):
        self.get_ops = get_ops
        self.get_display = get_display
        self.refresh = refresh
        self.job_factory = job_factory
        self.jobs = {}

    def start(self, operation, args=(), kwargs=None, expected_revision=None, document_id=None):
        ops = self.get_ops()
        doc = ops.doc
        with doc._lock:
            self.poll()
            if expected_revision is not None and (type(expected_revision) is not int or expected_revision != doc.revision):
                from .experiments import RevisionConflict
                raise RevisionConflict('Component start revision changed; your draft is preserved. Refresh and retry.')
            if document_id is not None and document_id != doc.document_id:
                from .experiments import RevisionConflict
                raise RevisionConflict('Component start document changed; your draft is preserved.')
            if any(job.state in ACTIVE for job in self.jobs.values()):
                raise KernelError('A component rebuild is already in progress')
            job = self.job_factory(doc, operation, args, kwargs)
            job.prepare_display = self.get_display() is not None
            self.jobs[job.id] = job
            try:
                job.start()
            except Exception:
                self.jobs.pop(job.id)
                raise
            return job.status()

    def poll(self):
        ops = self.get_ops()
        with ops.doc._lock:
            for job in self.jobs.values():
                if job.state not in ACTIVE:
                    continue
                job.poll()
                if job.state == 'ready':
                    try:
                        job.commit(ops, self.get_display())
                    except Exception as error:
                        job.fail(str(error))
                    if job.state == 'applied':
                        self.refresh()
            return [job.status() for job in self.jobs.values()]

    def discover(self):
        """Drain preparation without publishing: lost-start recovery must allow cancel first."""
        with self.get_ops().doc._lock:
            for job in self.jobs.values():
                if job.state in ACTIVE:
                    job.poll()
            return [job.status() for job in self.jobs.values()]

    def status(self, identity, cancel=False):
        with self.get_ops().doc._lock:
            job = self.jobs[identity]
            # Cancellation is recorded before polling, including a queued ready result.
            if cancel:
                job.cancel()
            self.poll()
            return job.status()

    def cancel_all(self):
        with self.get_ops().doc._lock:
            for job in self.jobs.values():
                job.cancel()


def owner(ops, get_ops=None, get_display=None, refresh=None):
    """Ops is the shared attachment point, available before a REST server exists."""
    if not hasattr(ops, '_component_jobs'):
        ops._component_jobs = ComponentJobService(get_ops or (lambda: ops))
    service = ops._component_jobs
    if get_ops is not None: service.get_ops = get_ops
    if get_display is not None: service.get_display = get_display
    if refresh is not None: service.refresh = refresh
    return service
