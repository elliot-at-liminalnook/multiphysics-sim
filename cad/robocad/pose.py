"""Non-destructive forward kinematics in CAD world coordinates (mm, radians)."""
import math
import numpy as np
from .kernel import KernelError

MOVABLE = ('revolute', 'continuous', 'prismatic')


def joint_range(j):
    span = j.stroke or 100. if j.type == 'prismatic' else math.pi
    lower = j.lower if j.lower is not None else j.home - span
    upper = j.upper if j.upper is not None else j.home + span
    if not all(math.isfinite(x) for x in (lower, upper, j.home)) or lower > upper:
        raise KernelError('Joint limits must be finite and ordered')
    return lower, upper


def joint_motion(j, value):
    delta = value - j.home
    matrix = np.eye(4)
    if j.type == 'fixed': return matrix
    axis = np.asarray(j.axis, dtype=float)
    length = np.linalg.norm(axis)
    if not np.isfinite(axis).all() or length < 1e-12: raise KernelError('Joint axis must be nonzero and finite')
    axis /= length
    if j.type == 'prismatic':
        matrix[:3, 3] = axis * delta
    else:
        x,y,z = axis
        cross = np.array([[0,-z,y],[z,0,-x],[-y,x,0]])
        rotation = np.eye(3) + math.sin(delta)*cross + (1-math.cos(delta))*(cross@cross)
        pivot = np.asarray(j.pivot, dtype=float)
        if not np.isfinite(pivot).all(): raise KernelError('Joint pivot must be finite')
        matrix[:3,:3] = rotation
        matrix[:3,3] = pivot - rotation@pivot
    return matrix


class PoseModel:
    """Tree transforms plus ideal transmissions and motor-driven closed hinges.

    No geometry queries: only immutable joint coordinates are used. Preview
    bounds are an interaction envelope, never inferred collision limits.
    """
    def __init__(self, doc):
        from copy import deepcopy
        self.doc = doc
        self.joints = {n.id:deepcopy(n.joint) for n in doc.nodes.values() if n.joint is not None and not n.disabled}
        self.names = {nid:doc.nodes[nid].name for nid in self.joints}
        self.nodes = tuple(doc.nodes)
        self.parents, self.loops = {}, {}
        for jid,j in self.joints.items():
            if j.type not in (*MOVABLE, 'fixed', 'loop_revolute', 'loop_spherical'):
                raise KernelError(f'{self.names[jid]}: unsupported preview joint {j.type}')
            if j.child not in doc.nodes or (j.parent is not None and j.parent not in doc.nodes):
                raise KernelError(f'{self.names[jid]}: a connected part is missing')
            if j.type.startswith('loop_'):
                self.loops[jid] = j
                continue
            if j.child in self.parents: raise KernelError('A part has multiple parent joints')
            self.parents[j.child] = (j.parent, jid)
            if j.type in MOVABLE: joint_range(j)
        for n in doc.nodes.values():
            mount = (n.robot or {}).get('mounted_on')
            if mount is not None and n.id not in self.parents:
                if mount not in doc.nodes: raise KernelError(f'{n.name}: mounted part is missing')
                self.parents[n.id] = (mount, None)
        self.home = {jid:j.home for jid,j in self.joints.items() if j.type in MOVABLE}
        self._forward(self.home)  # detect cycles
        for n in doc.nodes.values():
            if n.name.lower() == 'ground' or (n.robot or {}).get('ground'):
                for jid in self._ancestors(n.id):
                    if self.joints[jid].type in MOVABLE:
                        raise KernelError(f'{n.name}: a grounded part is connected below a moving joint')
        self.transmissions = {}
        for t in doc.robot_settings.get('transmissions', []):
            driver, driven, ratio = t['driver_joint'], t['driven_joint'], t['ratio']
            if driver not in self.home or driven not in self.home or any(self.joints[i].type == 'prismatic' for i in (driver, driven)):
                raise KernelError('Transmission requires two rotational joints')
            if driven in self.transmissions or not math.isfinite(ratio) or ratio == 0:
                raise KernelError('Transmission must have one driver and a finite nonzero ratio')
            self.transmissions[driven] = (driver, ratio)
        self._coupled(dict(self.home))  # detect transmission cycles
        self.loop_variables = {}
        for lid,j in self.loops.items():
            path = set(self._ancestors(j.parent)) ^ set(self._ancestors(j.child))
            moving = path.intersection(self.home)
            if not any(self.joints[i].motor for i in moving):
                raise KernelError('Closed-loop constraint solver needs a declared motor on the loop')
            self.loop_variables[lid] = sorted(i for i in moving if not self.joints[i].motor and i not in self.transmissions)
        self.passive = set(i for ids in self.loop_variables.values() for i in ids)
        self.drivers = {i:v for i,v in self.home.items() if i not in self.passive and i not in self.transmissions}
        self.last_positions = dict(self.home)
        self.last_error_mm = 0.

    def _ancestors(self, nid):
        result = []
        while nid in self.parents:
            nid,jid = self.parents[nid]
            if jid: result.append(jid)
        return result

    def _coupled(self, values):
        visiting, done = set(), set()
        def visit(i):
            if i in done or i not in self.transmissions: return
            if i in visiting: raise KernelError('Transmission cycle')
            visiting.add(i)
            driver,ratio = self.transmissions[i]; visit(driver)
            values[i] = self.home[i] + (values[driver]-self.home[driver])/ratio
            visiting.remove(i); done.add(i)
        for i in self.transmissions: visit(i)
        return values

    def _forward(self, values, requested=None):
        result, visiting = {}, set()
        identity = np.eye(4)
        def visit(nid):
            if nid is None: return identity
            if nid in result: return result[nid]
            if nid in visiting: raise KernelError('Joint or mounting cycle')
            visiting.add(nid)
            parent,jid = self.parents.get(nid, (None,None))
            matrix = visit(parent)
            if jid:
                j = self.joints[jid]
                if j.type != 'fixed': matrix = matrix @ joint_motion(j,values[jid])
            visiting.remove(nid); result[nid] = matrix
            return matrix
        for nid in (self.nodes if requested is None else requested): visit(nid)
        return result

    def _residual(self, values, lids):
        needed = {n for lid in lids for n in (self.loops[lid].parent,self.loops[lid].child) if n}
        matrices = self._forward(values, needed)
        result = []
        for lid in lids:
            j = self.loops[lid]; p = np.array(j.pivot); axis = np.array(j.axis,dtype=float); axis /= np.linalg.norm(axis)
            a,b = matrices.get(j.parent,np.eye(4)),matrices[j.child]
            result.extend(a[:3,:3]@p+a[:3,3]-b[:3,:3]@p-b[:3,3])
            if j.type == 'loop_revolute':
                result.extend(100.*(a[:3,:3]@axis-b[:3,:3]@axis))
        return np.array(result)

    def _bounds(self, jid):
        j = self.joints[jid]
        if jid in self.passive and j.type == 'prismatic':
            # A slider widget's default range is not a physical stop on a
            # constraint-driven coordinate. Respect only declared limits.
            return (j.lower if j.lower is not None else -math.inf,
                    j.upper if j.upper is not None else math.inf)
        return joint_range(j)

    def matrices(self, positions):
        # Large timeline jumps must follow the existing assembly branch.
        # Solving a folded crank from the neutral guess can otherwise put the
        # connecting rod on the opposite side of its pin, despite zero residual.
        if not self.loops: return self._solve(positions)
        if set(positions)-set(self.home): raise KernelError('Pose refers to a missing or unsupported joint')
        if any(not isinstance(v,(int,float)) or not math.isfinite(v) for v in positions.values()): raise KernelError('Pose values must be finite')
        before, error = dict(self.last_positions), self.last_error_mm
        target = {**self.home, **positions}
        steps = max([1]+[math.ceil(abs(target[i]-before[i])/(10. if self.joints[i].type=='prismatic' else math.radians(10))) for i in self.drivers])
        if steps > 360: raise KernelError('Motion jump is too large; use intermediate keyframes')
        try:
            for step in range(1,steps+1):
                values = {**target, **{i:before[i]+(target[i]-before[i])*step/steps for i in self.drivers}}
                result = self._solve(values)
            return result
        except Exception:
            self.last_positions, self.last_error_mm = before, error
            raise

    def _solve(self, positions):
        from scipy.optimize import least_squares
        if set(positions)-set(self.home): raise KernelError('Pose refers to a missing or unsupported joint')
        if any(not isinstance(v,(int,float)) or not math.isfinite(v) for v in positions.values()): raise KernelError('Pose values must be finite')
        values = self._coupled({**self.home, **positions})
        # Supplied passive coordinates are only initial guesses; closure is authoritative.
        at_home = all(abs(values[i]-self.home[i]) < 1e-12 for i in self.drivers)
        for i in self.passive: values[i] = self.home[i] if at_home else self.last_positions[i]
        for lid,variables in self.loop_variables.items():
            if not variables or np.max(np.abs(self._residual(values,[lid]))) < 1e-8: continue
            lo,hi = zip(*(self._bounds(i) for i in variables))
            def residual(x):
                trial = {**values, **dict(zip(variables,x))}
                return self._residual(self._coupled(trial),[lid])
            margin = np.minimum((np.array(hi)-np.array(lo))*.001, .001)
            x = np.clip([values[i] for i in variables],np.array(lo)+margin,np.array(hi)-margin)
            fit = least_squares(residual,x,bounds=(lo,hi),x_scale='jac',max_nfev=35,ftol=1e-9,xtol=1e-9,gtol=1e-9)
            if np.max(np.abs(fit.fun)) > .02:
                # Scrubbing may jump across a linkage's dead centre. Retry
                # from the imported branch instead of accepting a local minimum.
                seed = np.clip([self.home[i] for i in variables],np.array(lo)+margin,np.array(hi)-margin)
                retry = least_squares(residual,seed,bounds=(lo,hi),x_scale='jac',max_nfev=35,ftol=1e-9,xtol=1e-9,gtol=1e-9)
                if np.linalg.norm(retry.fun) < np.linalg.norm(fit.fun): fit = retry
            values.update(zip(variables,fit.x))
        error = float(np.max(np.abs(self._residual(values,self.loops)))) if self.loops else 0.
        if error > .02: raise KernelError(f'Linkage cannot close at this pose ({error:.3f} mm residual); playback stopped')
        for jid,v in values.items():
            lo,hi = self._bounds(jid)
            if not math.isfinite(v) or not lo-1e-9 <= v <= hi+1e-9: raise KernelError('Pose exceeds joint preview bounds')
        result = self._forward(values)
        self.last_positions = values
        self.last_error_mm = error
        return result
