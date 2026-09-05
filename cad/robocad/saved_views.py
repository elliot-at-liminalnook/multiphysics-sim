"""Named inspection views, persisted with the document and undoable."""
from copy import deepcopy
import math

from .annotations import camera_view
from .kernel import KernelError, Plane


def validate_state(state):
    if not isinstance(state, dict):
        raise KernelError('View state must be an object')
    camera_keys = {'target', 'distance', 'yaw', 'pitch', 'fov', 'orthographic', 'mode', 'rot'}
    if set(state) - camera_keys - {'section', 'grid', 'display_mode', 'comment_pins'}:
        raise KernelError('Unsupported saved view field')
    defaults = {'target': [0, 0, 0], 'distance': 250, 'yaw': -35, 'pitch': 28,
                'fov': 40, 'orthographic': False, 'mode': 'turntable',
                'rot': [[1, 0, 0], [0, 1, 0], [0, 0, 1]]}
    out = camera_view({**defaults, **{k: v for k, v in state.items() if k in camera_keys}})
    out['target'] = list(out['target'])
    out['rot'] = [list(row) for row in out['rot']]
    if not -89.5 <= out['pitch'] <= 89.5:
        raise KernelError('View pitch must be between -89.5 and 89.5 degrees')
    for key in ('grid', 'comment_pins'):
        if key in state and not isinstance(state[key], bool):
            raise KernelError(f'{key} must be boolean')
        out[key] = state.get(key, True)
    mode = state.get('display_mode', 'shaded_edges')
    if mode not in ('shaded', 'shaded_edges', 'wireframe', 'xray', 'matcap', 'render'):
        raise KernelError('Unknown display mode')
    out['display_mode'] = mode
    section = state.get('section', {'enabled': False, 'plane': None})
    if not isinstance(section, dict) or set(section) - {'enabled', 'plane'} or not isinstance(section.get('enabled', False), bool):
        raise KernelError('Section requires enabled and an optional plane')
    plane = section.get('plane')
    if plane is not None:
        if not isinstance(plane, dict) or set(plane) != {'origin', 'normal', 'x_axis'}:
            raise KernelError('Section plane requires origin, normal and x_axis')
        for value in plane.values():
            if not isinstance(value, (list, tuple)) or len(value) != 3 or not all(isinstance(x, (float, int)) and not isinstance(x, bool) and math.isfinite(x) for x in value):
                raise KernelError('Section plane needs finite 3D vectors')
        def unit(v):
            size = math.sqrt(sum(x*x for x in v))
            if size < 1e-10: raise KernelError('Section axes must be nonzero')
            return [x / size for x in v]
        normal, x_axis = unit(plane['normal']), unit(plane['x_axis'])
        if abs(sum(a*b for a, b in zip(normal, x_axis))) > 1e-6:
            raise KernelError('Section axes must be perpendicular')
        plane = {'origin': list(plane['origin']), 'normal': normal, 'x_axis': x_axis}
    if section.get('enabled') and plane is None:
        raise KernelError('An enabled section needs a plane')
    out['section'] = {'enabled': section.get('enabled', False), 'plane': plane}
    return deepcopy(out)


def capture_view(vp):
    cam = vp.camera
    return validate_state({
        **{k: getattr(cam, k) for k in ('target', 'distance', 'yaw', 'pitch', 'fov', 'orthographic', 'mode')},
        'rot': cam.rot.tolist(), 'grid': vp.show_grid, 'display_mode': vp.display_mode,
        'comment_pins': vp.show_comment_pins,
        'section': {'enabled': vp.section_enabled, 'plane': vp.section_plane.to_json() if vp.section_plane else None},
    })


def restore_view(app, state):
    # Validate everything before touching the live camera. No geometry queries.
    state = validate_state(state)
    import numpy as np
    vp = app.viewport
    vp.inspection_ids = None
    for key in ('distance', 'yaw', 'pitch', 'fov', 'orthographic', 'mode'):
        setattr(vp.camera, key, state[key])
    vp.camera.target = tuple(state['target'])
    vp.camera.rot = np.array(state['rot'], dtype=float)
    vp.section_enabled = state['section']['enabled']
    vp.section_plane = Plane.from_json(state['section']['plane']) if state['section']['plane'] else None
    vp.show_grid = state['grid']
    vp.show_comment_pins = state['comment_pins']
    app.set_display_mode(state['display_mode'])
    vp.update()


class ChangeSavedViews:
    def __init__(self, label, views):
        self.label, self.views, self.previous = label, deepcopy(views), None

    def apply(self, doc, views):
        doc.saved_views = deepcopy(views)
        doc.dirty = True
        doc.notify('saved_views')

    def do(self, doc):
        self.previous = deepcopy(doc.saved_views)
        self.apply(doc, self.views)

    def undo(self, doc):
        self.apply(doc, self.previous)

    def redo(self, doc):
        self.apply(doc, self.views)


class SavedViewOps:
    def saved_views(self):
        return deepcopy(list(self.doc.saved_views.values()))

    def save_view(self, name: str, state: dict) -> str:
        name = self._view_name(name)
        state = validate_state(state)
        vid = self.doc.new_id()
        views = deepcopy(self.doc.saved_views)
        views[vid] = {'id': vid, 'name': name, 'state': state}
        self.stack.push(ChangeSavedViews('Save view', views))
        return vid

    def update_saved_view(self, view_id: str, name=None, state=None):
        views = deepcopy(self.doc.saved_views)
        view = views[view_id]
        if name is not None: view['name'] = self._view_name(name)
        if state is not None: view['state'] = validate_state(state)
        self.stack.push(ChangeSavedViews('Update saved view', views))
        return view_id

    def delete_saved_view(self, view_id: str):
        views = deepcopy(self.doc.saved_views)
        del views[view_id]
        self.stack.push(ChangeSavedViews('Delete saved view', views))

    @staticmethod
    def _view_name(name):
        if not isinstance(name, str) or not name.strip() or len(name.strip()) > 120:
            raise KernelError('View name must contain 1–120 characters')
        return name.strip()
