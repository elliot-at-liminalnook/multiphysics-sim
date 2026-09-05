"""Undoable reference images, using embedded bytes as the source of truth."""
import io
import math
import os
from PIL import Image
from .document import Node
from .kernel import KernelError, Plane


class ReferenceOps:
    def import_references(self, paths, plane=None):
        from .commands import AddNodes
        plane = Plane.from_json(plane) if isinstance(plane, dict) else plane or Plane.xy()
        nodes = []
        for path in paths:
            with open(path, 'rb') as f:
                data = f.read()
            with Image.open(io.BytesIO(data)) as image:
                image.load()
                width, height = image.size
            node = Node(self.doc.new_id(), 'image', self.doc.unique_name(os.path.basename(path)),
                        locked=True, image={'path': str(path), 'data': data, 'plane': plane,
                        'width': 100., 'height': 100. * height / width, 'opacity': .6, 'rotation_deg': 0.})
            if any(n.name == node.name for n in nodes):
                node.name = f'{node.name} ({len(nodes)+1})'
            nodes.append(node)
        if nodes:
            self.stack.push(AddNodes('Import references', nodes))
        return [n.id for n in nodes]

    def update_reference(self, node_id, width=None, opacity=None, origin=None, plane=None,
                         rotation_deg=None, visible=None, locked=None, name=None):
        from .commands import SetAttributes
        node = self.doc.nodes[node_id]
        if node.image is None:
            raise KernelError('Select a reference image')
        image = dict(node.image)
        if width is not None:
            if not math.isfinite(width) or width <= 0: raise KernelError('Width must be positive')
            image['height'] *= width / image['width']
            image['width'] = width
        if opacity is not None:
            if not math.isfinite(opacity) or not 0 <= opacity <= 1: raise KernelError('Opacity must be between 0 and 1')
            image['opacity'] = opacity
        p = image['plane']
        if plane is not None:
            p = Plane.from_json(plane) if isinstance(plane, dict) else plane
            image['rotation_deg'] = 0.
        if origin is not None:
            if len(origin) != 3 or not all(math.isfinite(x) for x in origin): raise KernelError('Origin needs three finite coordinates')
            p = Plane(tuple(origin), p.normal, p.x_axis)
        if rotation_deg is not None:
            if not math.isfinite(rotation_deg): raise KernelError('Rotation must be finite')
            a = math.radians(rotation_deg - image.get('rotation_deg', 0.))
            x = tuple(math.cos(a)*u + math.sin(a)*v for u,v in zip(p.x_axis, p.y_axis))
            p = Plane(p.origin, p.normal, x)
            image['rotation_deg'] = rotation_deg
        image['plane'] = p
        changes = {'image': image}
        for k,v in [('visible', visible), ('locked', locked), ('name', name)]:
            if v is not None: changes[k] = v
        self.stack.push(SetAttributes('Edit reference', {node_id: changes}))
        return node_id

    def calibrate_reference(self, node_id, first, second, distance):
        """World-space calibration, keeping the first picked point stationary."""
        if len(first) != 3 or len(second) != 3 or not all(math.isfinite(v) for v in [*first, *second, distance]):
            raise KernelError('Calibration coordinates and distance must be finite')
        current = math.dist(first, second)
        if current < 1e-9 or distance <= 0: raise KernelError('Pick two distinct points and enter a positive distance')
        image = self.doc.nodes[node_id].image
        factor = distance / current
        origin = [a + factor*(o-a) for a,o in zip(first, image['plane'].origin)]
        return self.update_reference(node_id, width=image['width']*factor, origin=origin)
