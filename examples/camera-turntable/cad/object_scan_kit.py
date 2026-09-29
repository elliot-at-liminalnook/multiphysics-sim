"""Object-scanning kit for the camera turntable (mm, Z up, turntable axis on Z).

    cad/.venv/bin/python -c "from robocad.client import RoboClient; print(RoboClient().script('examples/camera-turntable/cad/object_scan_kit.py'))"

For scanning objects, the object sits on a platform on the disc and the
camera looks in from a fixed stand beside the turntable. The camera and the
ESP32 move from the disc to the stand, so no cable winds up. Take the camera
mount off the disc (two M3 screws).

* Platform: a Ø160 mm printed plate on three standoffs above the slip-ring
  clamp, screwed to the disc through the carrier screw circle.
* Stand: a base plate that sits on the table (weigh it down or tape it), a
  post, and a camera head on an M3 pivot with two detent holes. It gives a
  low ring (15° down) and a high ring (35° down), both aimed at the platform
  centre. Each detent has a reference camera body carrying its optics, from
  which derive_physics.py exports object-rig-low.json and object-rig-high.json.

In the object's frame the camera orbits the other way, which is why those rig
files have their axis reversed. Use a plain backdrop and even, diffuse
light: in the object's frame the room and a directional light turn, and
matching assumes they don't.
"""
import math

from robocad.kernel import BooleanOp

DEFAULTS = {'platform_diameter': 160.0, 'platform_height': 84.0, 'stand_distance': 250.0, 'object_center_height': 40.0,
            'low_tilt_deg': 15.0, 'high_tilt_deg': 35.0, 'stand_angle_deg': 0.0}
CAMERA = {'w': 25.0, 'h': 24.0, 'lens_len': 11.4, 'mass': 0.004}
M3 = 3.4


def build(ops, params):
    p = {**DEFAULTS, **params}
    doc, k = ops.doc, ops.doc.kernel
    made = {}

    def cyl(r, z0, z1, x=0.0, y=0.0):
        return k.cylinder((x, y, z0), (0, 0, 1), r, z1 - z0)

    def box(x0, y0, z0, x1, y1, z1):
        return k.box((min(x0, x1), min(y0, y1), min(z0, z1)), (abs(x1 - x0), abs(y1 - y0), abs(z1 - z0)))

    def U(a, *bs):
        for b in bs: a = k.boolean(a, b, BooleanOp.UNION)
        return a

    def D(a, *bs):
        for b in bs: a = k.boolean(a, b, BooleanOp.SUBTRACT)
        return a

    def add(key, body, name, material, color, meta):
        n = doc.add_body(body, name, material)
        n.color = color
        n.robot = meta
        made[key] = n.id
        return n

    top = p['platform_height']
    z_disc_top = 58.0
    # Platform on three standoffs over the slip-ring clamp.
    plate = cyl(p['platform_diameter'] / 2, top - 4.0, top)
    for a in (30, 150, 270):
        x, y = 32 * math.cos(math.radians(a)), 32 * math.sin(math.radians(a))
        plate = U(plate, D(cyl(5.0, z_disc_top, top - 4.0, x, y), cyl(M3 / 2, z_disc_top - 1, top + 1, x, y)))
        plate = D(plate, cyl(3.2, top - 2.0, top + 1, x, y))  # screw-head counterbore
    add('platform', plate, 'Object platform (on the disc)', 'pla', (0.95, 0.95, 0.93),
        {'print': {'role': 'object platform', 'material': 'PLA', 'status': 'designed, not yet printed',
                   'notes': 'Three M3×30 through the standoffs into the carrier screw circle. Print white or matte; a textured top helps matching.'}})

    # Stand: base, post, and two detent camera positions aimed at the object centre.
    sa = math.radians(p['stand_angle_deg'])
    R = p['stand_distance']
    target = (0.0, 0.0, top + p['object_center_height'])
    base = box(R - 50, -40, 0, R + 50, 40, 6)
    post_h = 0.0
    cams = {}
    for ring, tilt in (('low', p['low_tilt_deg']), ('high', p['high_tilt_deg'])):
        # Camera on the line from the target, `tilt` below horizontal, at the stand distance horizontally.
        t = math.radians(tilt)
        horizontal = R - 30.0  # lens front sits 30 mm in front of the post
        z = target[2] + horizontal * math.tan(t)
        cams[ring] = ((horizontal, 0.0, z), (-math.cos(t), 0.0, -math.sin(t)), tilt)
        post_h = max(post_h, z + 20.0)
    post = box(R - 8, -8, 6, R + 8, 8, post_h)
    for ring, ((cx, cy, cz), axis, tilt) in cams.items():
        post = D(post, k.cylinder((R - 9, 0, cz), (1, 0, 0), M3 / 2, 18))  # detent hole
    stand = U(base, post)
    add('stand', stand, 'Camera stand (fixed, beside the turntable)', 'pla', (0.2, 0.46, 0.72),
        {'print': {'role': 'camera stand', 'material': 'PLA', 'status': 'designed, not yet printed',
                   'notes': 'Two detent holes: low ring and high ring. The camera head pivots on an M3 bolt; the ESP32 board screws to the base plate.'}})
    for ring, ((cx, cy, cz), axis, tilt) in cams.items():
        # Reference camera body at this detent: board behind the lens, lens facing the target.
        body = box(cx, -CAMERA['w'] / 2, cz - CAMERA['h'] / 2, cx + 1.0, CAMERA['w'] / 2, cz + CAMERA['h'] / 2)
        body = U(body, k.cylinder((cx, 0, cz), (-1, 0, 0), 6.0, CAMERA['lens_len']))
        body = k.transform(body, rotation_axis=(0, 1, 0), rotation_deg=tilt, rotation_center=(cx, 0, cz))
        lens = (cx + axis[0] * CAMERA['lens_len'], 0.0, cz + axis[2] * CAMERA['lens_len'])
        add(f'camera_{ring}', body, f'Object camera ({ring} ring, reference)', 'pcb', (0.1, 0.42, 0.24),
            {'purchased': {'item': 'Raspberry Pi Camera Module 3 Wide (the one from the disc)', 'status': 'owned'},
             'optics': {'optical_center_mm': [lens[0] * math.cos(sa), lens[0] * math.sin(sa), lens[2]],
                        'optical_axis': [axis[0] * math.cos(sa), axis[0] * math.sin(sa), axis[2]],
                        'ring': ring, 'tilt_down_deg': tilt, 'aim_mm': list(target)},
             'measure_before_print': 'tilt of each detent after assembly (the rig file assumes the nominal angle)'})
    if p['stand_angle_deg']:
        ops.transform([made['stand'], made['camera_low'], made['camera_high']], axis=(0, 0, 1), angle_deg=p['stand_angle_deg'], center=(0, 0, 0))
    group = ops.group(list(made.values()), name='Object-scanning kit')
    return {'nodes': made, 'group': group, 'target_mm': list(target),
            'cameras': {r: {'center_mm': list(c[0]), 'axis': list(c[1]), 'tilt_deg': c[2]} for r, c in cams.items()}}
