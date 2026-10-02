"""Validate a caller-owned reference pose before restoring its solver branch.

No sessions or global seeds: preview/export callers keep separate published
receipts. Validation uses PoseModel's own coupling, bounds and closure residual;
it never projects arbitrary supplied passive coordinates onto another branch.
"""
import math
from .kernel import KernelError
from .experiments import RevisionConflict


def restore_prior(model, prior, expected_identity):
    if prior is None:
        return
    if not isinstance(prior, dict) or set(prior) != {'identity', 'positions'}:
        raise KernelError('motion.prior: require identity and full resolved positions')
    if prior['identity'] != expected_identity:
        raise RevisionConflict('motion.prior.identity: document, revision or kinematic source changed; reset continuation')
    stamp = prior['identity']
    if type(stamp.get('revision')) is not int:
        raise KernelError('motion.prior.identity.revision: require an integer')
    positions = prior['positions']
    if not isinstance(positions, dict) or set(positions) != set(model.home):
        raise KernelError('motion.prior.positions: require every resolved movable joint exactly once')
    if any(type(v) not in (int, float) or not math.isfinite(v) for v in positions.values()):
        raise KernelError('motion.prior.positions: values must be finite radians/mm')
    values = {jid: float(v) for jid, v in positions.items()}
    for jid, value in values.items():
        lo, hi = model._bounds(jid)
        if not lo - 1e-9 <= value <= hi + 1e-9:
            raise KernelError(f'motion.prior.positions.{jid}: exceeds reference bounds')
    coupled = model._coupled(dict(values))
    for jid, value in values.items():
        if not math.isclose(value, coupled[jid], rel_tol=0., abs_tol=1e-9):
            raise KernelError(f'motion.prior.positions.{jid}: inconsistent reference transmission')
    residual = model._residual(values, model.loops) if model.loops else ()
    if any(not math.isfinite(float(v)) for v in residual):
        raise KernelError('motion.prior.positions: non-finite reference closure residual')
    error = max((abs(float(v)) for v in residual), default=0.)
    if error > .02:
        raise KernelError(f'motion.prior.positions: reference linkage does not close ({error:.3f} mm residual)')
    # Publish to this request's model only after every check succeeds. The
    # next matrices() call uses exactly the persistent Qt continuation seed.
    model.last_positions = values
    model.last_error_mm = error
