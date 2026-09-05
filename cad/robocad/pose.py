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
    def __init__(self, doc):
        self.doc = doc
        self.joints = {n.id:n.joint for n in doc.nodes.values() if n.joint is not None and not n.disabled}
        self.parents = {}
        for jid,j in self.joints.items():
            if j.type not in (*MOVABLE, 'fixed'):
                raise KernelError(f'{doc.nodes[jid].name}: pose preview supports fixed, hinge and sliding joints; {j.type} needs a constraint solver')
            if j.child not in doc.nodes or (j.parent is not None and j.parent not in doc.nodes):
                raise KernelError(f'{doc.nodes[jid].name}: a connected part is missing')
            if j.child in self.parents: raise KernelError('A part has multiple parent joints; pose preview needs a tree')
            self.parents[j.child] = (j.parent, jid)
            if j.type in MOVABLE: joint_range(j)
        for n in doc.nodes.values():
            mount = (n.robot or {}).get('mounted_on')
            if mount is not None and n.id not in self.parents:
                if mount not in doc.nodes: raise KernelError(f'{n.name}: mounted part is missing')
                self.parents[n.id] = (mount, None)
        self.home = {jid:max(joint_range(j)[0], min(j.home, joint_range(j)[1])) for jid,j in self.joints.items() if j.type in MOVABLE}
        self.matrices(self.home)  # detect cycles before opening preview
        for n in doc.nodes.values():
            if n.name.lower() == 'ground' or (n.robot or {}).get('ground'):
                nid = n.id
                while nid in self.parents:
                    nid,jid = self.parents[nid]
                    if jid and self.joints[jid].type in MOVABLE:
                        raise KernelError(f'{n.name}: a grounded part is connected below a moving joint')

    def matrices(self, positions):
        unknown = set(positions) - set(self.home)
        if unknown: raise KernelError('Pose refers to a missing or unsupported joint')
        values = {**self.home, **positions}
        for jid,v in values.items():
            lo,hi = joint_range(self.joints[jid])
            if not math.isfinite(v) or not lo-1e-9 <= v <= hi+1e-9: raise KernelError('Pose exceeds joint limits')
        result, visiting = {}, set()
        def visit(nid):
            if nid is None: return np.eye(4)
            if nid in result: return result[nid]
            if nid in visiting: raise KernelError('Joint or mounting cycle: pose preview needs a tree')
            visiting.add(nid)
            parent,jid = self.parents.get(nid, (None,None))
            matrix = visit(parent)
            if jid:
                j = self.joints[jid]
                matrix = matrix @ joint_motion(j, values.get(jid, j.home))
            visiting.remove(nid)
            result[nid] = matrix
            return matrix
        for nid in self.doc.nodes: visit(nid)
        return result
