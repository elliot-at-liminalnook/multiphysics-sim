"""Revision-guarded assembly metadata edits without copying B-rep archives."""
from copy import deepcopy
import math
from .kernel import KernelError

def configure_robot(ops, expected_revision, updates=None, joints=None, groups=None, moves=None):
    from .candidates import check_revision
    from .commands import AddNodes, Composite, MoveNode, SetAttributes
    from .document import Node
    from .robotics import Joint, JOINT_TYPES, MOTOR_LIBRARY
    check_revision(ops.doc, expected_revision)
    doc=ops.doc; updates=deepcopy(updates or {}); joints=deepcopy(joints or []); groups=deepcopy(groups or []); moves=dict(moves or {})
    new=[]; aliases={}; changes={}; joint_ids=[]
    for g in groups:
        if not isinstance(g.get('key'),str) or g['key'] in aliases or g['key'] in doc.nodes: raise KernelError('Group keys must be unique')
        parent=aliases.get(g.get('parent'),g.get('parent'))
        if parent is not None and parent not in doc.nodes and parent not in aliases.values():raise KernelError('Group parent is missing')
        if parent in doc.nodes and doc.nodes[parent].kind!='group':raise KernelError('Group parent must be a group')
        nid=doc.new_id();aliases[g['key']]=nid
        new.append(Node(nid,'group',g['name'],parent=parent))
    def resolve(nid):return aliases.get(nid,nid)
    def body(nid):
        if nid not in doc.nodes or doc.nodes[nid].kind not in ('body','sheet','instance'):raise KernelError(f'{nid}: connected body is missing')
    for nid,fields in updates.items():
        if nid not in doc.nodes:raise KernelError(f'{nid}: node is missing')
        if set(fields)-{'name','material','color','robot','disabled'}:raise KernelError('Assembly updates accept name, material, color, robot and disabled')
        if 'material' in fields and fields['material'] not in doc.materials:raise KernelError('Material is missing')
        meta=fields.get('robot')
        if meta is not None:
            if not isinstance(meta,dict):raise KernelError('robot metadata must be an object')
            if meta.get('mounted_on') is not None:body(meta['mounted_on'])
            if meta.get('kind')=='motor' and meta.get('spec') not in MOTOR_LIBRARY:raise KernelError('Unknown motor specification')
            from .physical import validate_mass_metadata
            validate_mass_metadata(meta,doc)
        changes[nid]=fields
    staged={n.id:n.joint for n in doc.nodes.values() if n.joint and not n.disabled}
    seen=set()
    for entry in joints:
        nid=entry.pop('id',None);parent_group=resolve(entry.pop('group',None));name=entry.pop('name',None)
        if parent_group is not None and parent_group not in aliases.values() and (parent_group not in doc.nodes or doc.nodes[parent_group].kind!='group'):raise KernelError('Joint group is missing')
        if nid:
            if nid in seen or nid not in staged:raise KernelError('Updated joint is missing or repeated')
            seen.add(nid);fields=staged[nid].to_json();fields.update(entry)
        else:fields=entry
        j=Joint.from_json(fields)
        if j.type not in JOINT_TYPES:raise KernelError('Unsupported joint type')
        body(j.child)
        if j.parent is not None:body(j.parent)
        if j.parent==j.child:raise KernelError('A joint cannot connect a body to itself')
        if len(j.pivot)!=3 or len(j.axis)!=3 or not all(math.isfinite(v) for v in (*j.pivot,*j.axis,j.gear_ratio,j.home)):raise KernelError('Joint coordinates must be finite 3-vectors')
        if sum(v*v for v in j.axis)<1e-16 or j.gear_ratio<=0:raise KernelError('Joint axis and gear ratio must be nonzero/positive')
        for v in (j.lower,j.upper):
            if v is not None and not math.isfinite(v):raise KernelError('Joint limits must be finite')
        if j.lower is not None and j.upper is not None and j.lower>j.upper:raise KernelError('Joint limits are reversed')
        if j.motor:
            body(j.motor);meta=changes.get(j.motor,{}).get('robot',doc.nodes[j.motor].robot) or {}
            if meta.get('kind')!='motor':raise KernelError('Assigned actuator is not a motor')
        if nid:
            changes.setdefault(nid,{})['joint']=j
            if name:changes[nid]['name']=name
            if parent_group:moves[nid]=parent_group
        else:
            nid=doc.new_id();new.append(Node(nid,'joint',name or f'{j.type} {doc.nodes[j.child].name}',parent=parent_group,joint=j))
        staged[nid]=j;joint_ids.append(nid)
    parents={};motors={}
    for nid,j in staged.items():
        if j.motor:
            if j.motor in motors:raise KernelError('A motor cannot drive multiple joints')
            motors[j.motor]=nid
            fields=changes.setdefault(j.motor,{})
            meta=deepcopy(fields.get('robot',doc.nodes[j.motor].robot) or {})
            meta['drives']=nid
            if meta.get('mounted_on') is None:meta['mounted_on']=j.parent
            fields['robot']=meta
        if not j.type.startswith('loop_'):
            if j.child in parents:raise KernelError('A body has multiple tree parents')
            parents[j.child]=j.parent
    for nid in doc.nodes:
        meta=changes.get(nid,{}).get('robot',doc.nodes[nid].robot) or {}
        if meta.get('mounted_on') is not None and nid not in parents:parents[nid]=meta['mounted_on']
    for nid in parents:
        seen=set();cur=nid
        while cur is not None and cur in parents:
            if cur in seen:raise KernelError('Assembly contains a tree cycle')
            seen.add(cur);cur=parents[cur]
    tree={n.id:n.parent for n in doc.nodes.values()};tree.update({n.id:n.parent for n in new})
    for nid,parent in moves.items():
        parent=resolve(parent)
        if nid not in tree or (parent is not None and parent not in tree):raise KernelError('Outliner move refers to a missing node')
        if parent in doc.nodes and doc.nodes[parent].kind!='group':raise KernelError('Outliner parent must be a group')
        tree[nid]=parent
    for nid in tree:
        seen=set();cur=nid
        while cur is not None:
            if cur in seen:raise KernelError('Outliner move creates a cycle')
            seen.add(cur);cur=tree[cur]
    commands=[]
    if new:commands.append(AddNodes('Configure assembly',new))
    if changes:commands.append(SetAttributes('Configure assembly',changes))
    commands.extend(MoveNode('Configure assembly',nid,resolve(parent)) for nid,parent in moves.items())
    if commands:ops.stack.push(Composite('Configure robot assembly',commands))
    return {'revision':doc.revision,'groups':aliases,'joints':joint_ids,'updated':list(changes)}
