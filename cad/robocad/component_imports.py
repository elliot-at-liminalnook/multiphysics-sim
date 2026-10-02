"""Freshness of imported structural metadata, distinct from measured results.

Graph edits affect result freshness but do not change geometry-derived port
structure. Legacy records lacking a derivation hash conservatively use the full
physical hash. Snapshot/file work belongs to HTTP or the metadata worker.
"""
from .snapshots import capture


def metadata_guard(doc, record, current=None):
    if doc is None:
        return {'metadata_stale': True, 'guard_document_id': None, 'guard_revision': None}
    current = current or capture(doc)
    provenance = record.get('provenance') or {}
    identity = record.get('document_id') or provenance.get('document_id')
    recorded = provenance.get('cad_derivation_hash')
    matches = bool(recorded) and recorded == current.cad_derivation_hash
    if not recorded:
        recorded = provenance.get('physical_hash')
        matches = bool(recorded) and recorded == current.physical_hash
    fresh = record.get('state') == 'completed' and identity == current.document_id and matches
    return {'metadata_stale': not fresh, 'guard_document_id': current.document_id,
            'guard_revision': current.revision}


def refresh_async(manager, run_id, token, emit):
    """Service worker; the Qt consumer receives data through its queued signal."""
    import threading
    def prepare():
        try: result = manager.components(run_id)
        except Exception as error: result = {'metadata_error': str(error), 'run_id': run_id}
        try: emit((token, result))
        except RuntimeError: pass  # Qt consumer was destroyed; no source mutation.
    threading.Thread(target=prepare, daemon=True, name='component-import-metadata').start()
