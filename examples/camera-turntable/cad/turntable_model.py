"""Camera turntable: parametric CAD model (mm, Z up, turntable axis on Z).

Run it in the live CAD window (one undo step; re-running replaces the last run):

    cad/.venv/bin/python -c "from robocad.client import RoboClient; print(RoboClient().script('examples/camera-turntable/cad/turntable_model.py'))"

or headless: ``cad/.venv/bin/python examples/camera-turntable/cad/build.py``.

A printed disc turns on two 6808-2RS bearings around a fixed printed post.
A GT2 ring (printed into the underside of the disc) is driven by a 30-tooth
printed pulley on the HX-30HM servo through a stock 610 mm closed GT2 loop;
the servo cradle slides radially to tension the belt. An Adafruit 736 slip
ring (22 mm, 6 wires, 2 A) through the post carries 5 V to the ESP32-P4 board
on the disc; the Camera Module 3 Wide looks outward from the rim.

Purchased parts are reference bodies with declared masses. Dimensions not
confirmed by a datasheet are marked ``measure before print`` in their
metadata (servo shaft position and horn, slip-ring body length, ESP32 board
outline and holes). Printed parts carry print intent; their masses come from
CAD volume × PLA density (solid), which over-states a sparse-infill print.
"""
import math

from robocad.belt_derivation import GT2, belt_path, pitch_radius, tip_radius, toothed_outline
from robocad.kernel import BooleanOp, Plane, Sketch
from robocad.physical import _inertia_about_com

DEFAULTS = {
    'ring_teeth': 280, 'pulley_teeth': 30, 'belt_length': 610.0, 'belt_width': 6.0,
    'disc_diameter': 300.0, 'disc_thickness': 4.0, 'base_diameter': 300.0, 'base_thickness': 5.0,
    'camera_angle_deg': 90.0, 'servo_angle_deg': 0.0,
}
# Purchased parts (nominal datasheet values unless marked).
BEARING = {'bore': 40.0, 'od': 52.0, 'width': 7.0, 'mass': 0.025, 'item': '6808-2RS deep-groove ball bearing (40×52×7), ×2'}
SLIP_RING = {'body_d': 22.0, 'body_len': 25.0, 'flange_d': 44.0, 'flange_t': 3.0, 'mass': 0.015,
             'item': 'Adafruit 736 slip ring with flange, 22 mm, 6 wires, 2 A (SRC022A-6)'}
SERVO = {'length': 45.2, 'width': 24.7, 'height': 35.0, 'shaft_from_end': 12.35, 'horn_d': 20.0, 'horn_t': 3.0,
         'mass': 0.052, 'item': 'Hiwonder HX-30HM serial bus servo'}
CAMERA = {'w': 25.0, 'h': 24.0, 'board_t': 1.0, 'lens_d': 12.0, 'lens_len': 11.4, 'holes': ((2.0, 2.0), (23.0, 2.0), (2.0, 14.5), (23.0, 14.5)),
          'lens_center': (12.5, 14.5), 'mass': 0.004, 'item': 'Raspberry Pi Camera Module 3 Wide (IMX708, 102° H FoV)'}
BOARD = {'l': 65.0, 'w': 30.0, 't': 1.6, 'holes': ((3.5, 3.5), (61.5, 3.5), (3.5, 26.5), (61.5, 26.5)), 'mass': 0.015,
         'item': 'ESP32-P4-WIFI6-M development board (Waveshare)'}
M3_CLEAR, M3_TAP = 3.4, 2.5


def build(ops, params):
    p = {**DEFAULTS, **params}
    doc, k = ops.doc, ops.doc.kernel
    made = {}

    # ------------------------------------------------------------ helpers
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

    def prism(points, z0, z1):
        sk = Sketch(Plane.xy(z0)); sk.polyline(points, closed=True)
        return k.extrude(sk.to_body(), (0, 0, 1), z1 - z0)

    def rot(body, deg):
        return k.transform(body, rotation_axis=(0, 0, 1), rotation_deg=deg, rotation_center=(0, 0, 0)) if deg else body

    def add(key, body, name, material, color, meta):
        n = doc.add_body(body, name, material)
        n.color = color
        n.robot = meta
        made[key] = n.id
        return n

    def declared(body, mass, source):
        pr = k.inertial_properties(body)
        dens = mass * 1000.0 / (pr.volume / 1000.0)  # g/cm³ that gives the declared mass
        _, inertia = _inertia_about_com(pr, dens)
        return {'mass_properties': {'mass_kg': mass, 'com_mm': list(pr.centroid), 'inertia_kg_m2': inertia.tolist(),
                                    'source': source}}

    def printed(role, notes, **extra):
        return {'print': {'role': role, 'material': 'PLA', 'status': 'designed, not yet printed', 'notes': notes, **extra}}

    def bought(spec, status, **extra):
        return {'purchased': {'item': spec['item'], 'status': status, **extra}}

    PLA_A, PLA_B, METAL, DARK, PCB = (0.93, 0.55, 0.16), (0.2, 0.46, 0.72), (0.72, 0.74, 0.78), (0.16, 0.17, 0.19), (0.1, 0.42, 0.24)

    # ------------------------------------------------------------ belt layout
    ring_r, pulley_r = pitch_radius(p['ring_teeth']), pitch_radius(p['pulley_teeth'])

    def path_at(c):
        return belt_path([{'center': (0.0, 0.0), 'radius': ring_r}, {'center': (c, 0.0), 'radius': pulley_r}])
    lo, hi = ring_r + pulley_r + 4.0, 200.0
    for _ in range(80):  # centre distance that fits the stock belt exactly
        mid = (lo + hi) / 2
        lo, hi = (mid, hi) if path_at(mid)['length'] < p['belt_length'] else (lo, mid)
    C = (lo + hi) / 2
    sa = math.radians(p['servo_angle_deg'])
    sx, sy = C * math.cos(sa), C * math.sin(sa)

    # ------------------------------------------------------------ heights
    zb = p['base_thickness']                      # base top
    z_servo_top = zb + SERVO['height']
    z_horn_top = z_servo_top + SERVO['horn_t']
    z_belt0 = z_horn_top + 1.0                    # pulley lower flange 1 mm
    z_belt1 = z_belt0 + p['belt_width'] + 1.0     # teeth band (belt + 1 mm)
    z_disc0 = z_belt1 + 1.0 + 2.0                 # upper flange, then 2 mm clearance
    z_disc1 = z_disc0 + p['disc_thickness']
    bw = BEARING['width']
    z_bear_lo = 12.0
    z_bear_hi = z_disc0 - bw
    post_r, bore_r, hub_r = BEARING['bore'] / 2, SLIP_RING['body_d'] / 2 + 0.5, BEARING['od'] / 2 + 0.05

    # ------------------------------------------------------------ base + post (one print)
    base = cyl(p['base_diameter'] / 2, 0.0, zb)
    base = U(base, cyl(post_r + 1.5, zb, z_bear_lo), cyl(post_r - 0.05, z_bear_lo, z_disc0))
    base = D(base, cyl(bore_r, -1.0, z_disc0 + 1.0))
    base = D(base, box(0.0, -3.0, -1.0, -p['base_diameter'] / 2 - 1, 3.0, 2.5))       # underside cable channel
    base = D(base, box(-p['base_diameter'] / 2 + 8, -6.0, -1.0, -p['base_diameter'] / 2 + 11, 6.0, zb + 1))  # zip-tie slot
    for a in (0, 120, 240):                                                             # cap screws
        base = D(base, cyl(M3_TAP / 2, z_disc0 - 10.0, z_disc0 + 1.0, (post_r - 4) * math.cos(math.radians(a + 30)), (post_r - 4) * math.sin(math.radians(a + 30))))
    # Servo cradle slots: two M4 slots along the radius for belt tension.
    for t in (-8.0, 26.0):
        cx, cy = sx + t * -math.sin(sa), sy + t * math.cos(sa)
        sk = Sketch(Plane.xy(-1.0)); sk.slot((cx - 5 * math.cos(sa), cy - 5 * math.sin(sa)), (cx + 5 * math.cos(sa), cy + 5 * math.sin(sa)), 4.4)
        base = D(base, k.extrude(sk.to_body(), (0, 0, 1), zb + 2))
    for a in range(45, 360, 90):                                                        # lightening windows
        if abs(((a - p['servo_angle_deg']) + 180) % 360 - 180) < 40: continue
        sk = Sketch(Plane.xy(-1.0)); sk.slot((60 * math.cos(math.radians(a)), 60 * math.sin(math.radians(a))), (115 * math.cos(math.radians(a)), 115 * math.sin(math.radians(a))), 30)
        base = D(base, k.extrude(sk.to_body(), (0, 0, 1), zb + 2))
    add('base', base, 'Base and bearing post', 'pla', PLA_B, printed('base', 'Print flat. Post outer Ø39.9 is a light press for the 6808 inner races; tune to your printer with the fit coupon.', ground=True))

    cap = D(cyl(post_r + 1.5, z_disc0, z_disc0 + 3.0), cyl(bore_r, z_disc0 - 1, z_disc0 + 4))
    for a in (0, 120, 240):
        cap = D(cap, cyl(M3_CLEAR / 2, z_disc0 - 1, z_disc0 + 4, (post_r - 4) * math.cos(math.radians(a + 30)), (post_r - 4) * math.sin(math.radians(a + 30))))
    add('cap', cap, 'Bearing retainer cap', 'pla', PLA_B, printed('retainer', 'Clamps the upper inner race; 3 × M3×8 into the post.'))

    # ------------------------------------------------------------ servo, cradle, pulley
    L, W, H, e = SERVO['length'], SERVO['width'], SERVO['height'], SERVO['shaft_from_end']
    # Servo long axis tangential (+t), shaft end first, so the body stays inside the disc radius.
    def local(x_rad, y_tan, z0, z1):  # box in the servo frame (radial, tangential) → world
        b = box(x_rad[0], y_tan[0], z0, x_rad[1], y_tan[1], z1)
        return rot(k.transform(b, translation=(C, 0.0, 0.0)), p['servo_angle_deg'])
    servo = local((-W / 2, W / 2), (-e, L - e), zb, z_servo_top)
    servo = U(servo, rot(cyl(3.0, z_servo_top, z_servo_top + 1.0, C, 0.0), p['servo_angle_deg']))
    add('servo', servo, 'HX-30HM servo (reference)', 'abs', DARK,
        {**bought(SERVO, 'owned'), **declared(servo, SERVO['mass'], 'manufacturer mass 52 g; uniform density over the envelope'),
         'measure_before_print': 'shaft position along the case (12.35 mm from the end is an estimate) and case size'})
    horn = rot(cyl(SERVO['horn_d'] / 2, z_servo_top, z_horn_top, C, 0.0), p['servo_angle_deg'])
    add('horn', horn, 'Servo horn (reference)', 'al', METAL,
        {'purchased': {'item': 'HX-30HM round metal horn (supplied with the servo)', 'status': 'owned'}, **declared(horn, 0.004, 'estimate'),
         'measure_before_print': 'horn diameter, thickness and screw circle'})

    cl, wall = 0.4, 3.0
    cradle = local((-W / 2 - cl - wall, W / 2 + cl + wall), (-e - cl - wall, L - e + cl + wall), zb, zb + 22.0)
    cradle = D(cradle, local((-W / 2 - cl, W / 2 + cl), (-e - cl, L - e + cl), zb + 3.0, zb + 30.0))
    cradle = U(cradle, local((-W / 2 - cl - wall - 6, W / 2 + cl + wall + 6), (-e - cl - wall, L - e + cl + wall), zb, zb + 3.0))
    for t in (-8.0, 26.0):  # M4 through the slots in the base
        cradle = D(cradle, rot(cyl(2.2, zb - 1, zb + 4, C, t), p['servo_angle_deg']))
    cradle = D(cradle, local((-5.0, 5.0), (L - e - 2.0, L - e + cl + wall + 1), zb + 6.0, zb + 30.0))  # cable exit
    add('cradle', cradle, 'Servo cradle (slides to tension the belt)', 'pla', PLA_A,
        printed('servo cradle', 'Two M4 bolts through the base slots; slide outward to tension the belt, then tighten.', clearance_mm=cl))

    pulley = prism(toothed_outline(p['pulley_teeth']), z_belt0, z_belt1)
    fl = tip_radius(p['pulley_teeth']) + 2.0
    pulley = U(pulley, cyl(fl, z_horn_top, z_belt0), cyl(fl, z_belt1, z_belt1 + 1.0))
    pulley = k.transform(pulley, translation=(C, 0.0, 0.0))
    pulley = D(pulley, cyl(1.6, z_horn_top - 1, z_belt1 + 2, C, 0.0))
    for a in (45, 135, 225, 315):
        pulley = D(pulley, cyl(M3_CLEAR / 2 - 0.3, z_horn_top - 1, z_belt1 + 2, C + 7 * math.cos(math.radians(a)), 7 * math.sin(math.radians(a))))
    pulley = rot(pulley, p['servo_angle_deg'])
    add('pulley', pulley, f"GT2 {p['pulley_teeth']}T drive pulley", 'pla', PLA_A,
        {**printed('drive pulley', 'Screws to the servo horn (4 × M2.5/M3 on a 14 mm circle: check your horn).'),
         'belt_pulley': {'drive': 'turntable', 'order': 1, 'teeth': p['pulley_teeth'], 'side': 'teeth', 'role': 'driver', 'pretension_n': 25.0, 'pretension_uncertainty': 0.5, 'pretension_provenance': 'estimated', 'pretension_source': 'Typical static tension per run for a 6 mm GT2 belt, set by sliding the servo cradle; measure with a belt tension gauge or the pluck frequency'}})

    # ------------------------------------------------------------ disc + ring + hub (one print, upside down)
    R_disc = p['disc_diameter'] / 2
    disc = cyl(R_disc, z_disc0, z_disc1)
    ring = prism(toothed_outline(p['ring_teeth']), z_belt0, z_disc0)
    ring = D(ring, cyl(tip_radius(p['ring_teeth']) - 7.0, z_belt0 - 1, z_disc0 + 1))
    hub = D(cyl(hub_r + 4.0, z_bear_lo, z_disc0), cyl(hub_r, z_bear_lo - 1, z_bear_lo + bw), cyl(hub_r, z_bear_hi, z_disc1 + 1),
            cyl(post_r + 3.0, z_bear_lo, z_disc0 + 1))
    disc = U(disc, ring, hub)
    disc = D(disc, cyl(hub_r, z_disc0 - 1, z_disc1 + 1))
    for a in (0, 90, 180, 270):  # carrier screws
        disc = D(disc, cyl(M3_TAP / 2, z_disc0, z_disc1 + 1, 32 * math.cos(math.radians(a + 45)), 32 * math.sin(math.radians(a + 45))))
    for a in range(0, 360, 60):  # spokes between the ring and the rim stay; windows lighten the web
        if abs(((a - p['camera_angle_deg']) + 180) % 360 - 180) < 50: continue
        sk = Sketch(Plane.xy(z_disc0 - 1)); sk.slot((45 * math.cos(math.radians(a)), 45 * math.sin(math.radians(a))), (70 * math.cos(math.radians(a)), 70 * math.sin(math.radians(a))), 18)
        disc = D(disc, k.extrude(sk.to_body(), (0, 0, 1), p['disc_thickness'] + 2))
    add('disc', disc, 'Turntable disc with GT2 ring and bearing hub', 'pla', PLA_A,
        {**printed('disc', 'Print upside down (disc face on the bed). Hub bore Ø52.1 takes both 6808 outer races; the internal rib locates them.'),
         'belt_pulley': {'drive': 'turntable', 'order': 0, 'teeth': p['ring_teeth'], 'side': 'teeth', 'role': 'driven'}})

    for i, z0 in enumerate((z_bear_lo, z_bear_hi)):
        b = D(cyl(BEARING['od'] / 2, z0, z0 + bw), cyl(BEARING['bore'] / 2, z0 - 1, z0 + bw + 1))
        add(f'bearing{i}', b, f"6808-2RS bearing ({'lower' if i == 0 else 'upper'})", 'steel', METAL,
            {**bought(BEARING, 'to buy'), **declared(b, BEARING['mass'], 'typical catalogue mass for 6808-2RS')})

    # ------------------------------------------------------------ slip ring
    z_carrier = z_disc1
    carrier = D(cyl(38.0, z_carrier, z_carrier + 3.0), cyl(bore_r, z_carrier - 1, z_carrier + 4))
    for a in (0, 90, 180, 270):
        carrier = D(carrier, cyl(M3_CLEAR / 2, z_carrier - 1, z_carrier + 4, 32 * math.cos(math.radians(a + 45)), 32 * math.sin(math.radians(a + 45))))
    add('carrier', carrier, 'Slip-ring carrier', 'pla', PLA_B, printed('slip-ring carrier', 'Bridges the disc centre; 4 × M3×8 into the disc.'))
    zf = z_carrier + 3.0
    clamp = D(cyl(30.0, zf, zf + SLIP_RING['flange_t'] + 2.0), cyl(SLIP_RING['flange_d'] / 2 + 0.3, zf - 1, zf + SLIP_RING['flange_t']),
              cyl(SLIP_RING['body_d'] / 2 - 2.0, zf - 1, zf + 10))
    for a in (0, 120, 240):
        clamp = D(clamp, cyl(M3_CLEAR / 2, zf - 1, zf + 10, 26 * math.cos(math.radians(a)), 26 * math.sin(math.radians(a))))
        carrier = D(carrier, cyl(M3_TAP / 2, z_carrier - 1, zf + 1, 26 * math.cos(math.radians(a)), 26 * math.sin(math.radians(a))))
    doc.nodes[made['carrier']].body = carrier
    add('clamp', clamp, 'Slip-ring flange clamp', 'pla', PLA_B,
        printed('flange clamp', 'Clamps the slip-ring flange to the carrier, so the flange hole pattern does not matter. 3 × M3×10.'))
    ring_body = U(cyl(SLIP_RING['body_d'] / 2, zf - SLIP_RING['body_len'], zf), cyl(SLIP_RING['flange_d'] / 2, zf, zf + SLIP_RING['flange_t']))
    add('slipring', ring_body, 'Slip ring (reference)', 'abs', METAL,
        {**bought(SLIP_RING, 'to buy', url='https://www.adafruit.com/product/736', price_usd=14.95),
         **declared(ring_body, SLIP_RING['mass'], 'estimate: capsule slip ring of this size'),
         'measure_before_print': 'body length below the flange (25 mm assumed); the post bore is through, so only clearance depends on it',
         'wiring': 'flange side turns with the disc (to the ESP32 5 V/GND); the other side\'s wires run down the post and are zip-tied at the base slot, which stops them turning'})

    # ------------------------------------------------------------ camera + ESP32 on the disc
    ca = p['camera_angle_deg']
    y_plate = R_disc - 10.0
    z_cam = z_disc1 + 26.0
    mount = box(-18.0, y_plate, z_disc1, 18.0, y_plate + 3.0, z_cam + 16.0)
    mount = U(mount, box(-18.0, y_plate - 22.0, z_disc1, 18.0, y_plate + 3.0, z_disc1 + 3.0))
    for gx in (-18.0, 15.0):  # side gussets
        sk = Sketch(Plane.yz(gx)); sk.polyline([(y_plate - 20.0, z_disc1 + 3.0), (y_plate, z_disc1 + 3.0), (y_plate, z_cam)], closed=True)
        mount = U(mount, k.extrude(sk.to_body(), (1, 0, 0), 3.0))
    cx0, cz0 = -CAMERA['w'] / 2, z_cam - CAMERA['lens_center'][1]
    for hx, hz in CAMERA['holes']:
        mount = U(mount, k.cylinder((cx0 + hx, y_plate + 3.0, cz0 + hz), (0, 1, 0), 2.2, 2.0))
        mount = D(mount, k.cylinder((cx0 + hx, y_plate - 1.0, cz0 + hz), (0, 1, 0), 0.9, 7.0))
    for sx_ in (-12.0, 12.0):
        mount = D(mount, cyl(M3_CLEAR / 2, z_disc1 - 1, z_disc1 + 4, sx_, y_plate - 12.0))
        disc = D(doc.nodes[made['disc']].body, cyl(M3_TAP / 2, z_disc0, z_disc1 + 1, sx_, y_plate - 12.0))
        doc.nodes[made['disc']].body = disc
    mount = D(mount, box(-8.0, y_plate - 1.0, z_disc1 + 3.0, 8.0, y_plate + 4.0, z_disc1 + 7.0))  # ribbon slot
    add('camera_mount', rot(mount, ca - 90.0), 'Camera mount', 'pla', PLA_B,
        printed('camera mount', 'Camera faces outward; M2 screws on 2 mm bosses. The ribbon passes through the slot to the ESP32.'))
    cam = box(cx0, y_plate + 5.0, cz0, cx0 + CAMERA['w'], y_plate + 5.0 + CAMERA['board_t'], cz0 + CAMERA['h'])
    cam = U(cam, k.cylinder((0.0, y_plate + 5.0 + CAMERA['board_t'], z_cam), (0, 1, 0), CAMERA['lens_d'] / 2, CAMERA['lens_len'] - CAMERA['board_t']))
    cam = rot(cam, ca - 90.0)
    add('camera', cam, 'Camera Module 3 Wide (reference)', 'pcb', PCB,
        {**bought(CAMERA, 'owned (2, one spare)'), **declared(cam, CAMERA['mass'], 'Raspberry Pi product brief (≈4 g)'),
         'optics': {'horizontal_fov_deg': 102.0, 'diagonal_fov_deg': 120.0, 'sensor': 'Sony IMX708, 4608 × 2592, rolling shutter',
                    'optical_axis': [math.cos(math.radians(ca)), math.sin(math.radians(ca)), 0.0],
                    'optical_center_mm': [(y_plate + 5.0 + CAMERA['lens_len']) * math.cos(math.radians(ca)), (y_plate + 5.0 + CAMERA['lens_len']) * math.sin(math.radians(ca)), z_cam]},
         'measure_before_print': 'lens centre height on the board (14.5 mm from the bottom edge assumed)'})

    yb0 = 52.0
    standoffs = None
    for hx, hy in BOARD['holes']:
        s = D(cyl(2.8, z_disc1, z_disc1 + 5.0, hy - BOARD['w'] / 2, yb0 + hx), cyl(1.1, z_disc1, z_disc1 + 6.0, hy - BOARD['w'] / 2, yb0 + hx))
        standoffs = s if standoffs is None else U(standoffs, s)
    add('standoffs', rot(standoffs, ca - 90.0), 'ESP32 board standoffs', 'pla', PLA_B,
        printed('standoffs', 'Glue or print onto the disc; M2.5 thread-forming screws.'))
    board = box(-BOARD['w'] / 2, yb0, z_disc1 + 5.0, BOARD['w'] / 2, yb0 + BOARD['l'], z_disc1 + 5.0 + BOARD['t'])
    board = U(board, box(-6.0, yb0 + BOARD['l'] - 1.0, z_disc1 + 5.0 + BOARD['t'], 6.0, yb0 + BOARD['l'] + 1.5, z_disc1 + 9.0))  # camera connector
    add('board', rot(board, ca - 90.0), 'ESP32-P4-WIFI6-M (reference)', 'pcb', PCB,
        {**bought(BOARD, 'owned (2, one spare)'), **declared(board, BOARD['mass'], 'estimate'),
         'measure_before_print': 'board outline and hole pattern (Pi Zero 65 × 30 mm, holes 58 × 23 mm assumed)'})

    # ------------------------------------------------------------ belt (reference)
    path = path_at(C)
    pts_in, pts_out = [], []
    circles = [((0.0, 0.0), ring_r), ((C, 0.0), pulley_r)]
    spans = path['spans']
    for i, (c, r) in enumerate(circles):
        a0 = math.atan2(spans[i - 1]['end'][1] - c[1], spans[i - 1]['end'][0] - c[0])
        wrap = path['wraps'][i]['angle']
        n = max(8, int(wrap * r / 1.5))
        for j in range(n + 1):
            a = a0 + wrap * j / n
            pts_in.append((c[0] + (r - 0.75) * math.cos(a), c[1] + (r - 0.75) * math.sin(a)))
            pts_out.append((c[0] + (r + 0.63) * math.cos(a), c[1] + (r + 0.63) * math.sin(a)))
    belt = D(prism(pts_out, z_belt0 + 0.5, z_belt0 + 0.5 + p['belt_width']), prism(pts_in, z_belt0, z_belt1 + 1))
    add('belt', rot(belt, p['servo_angle_deg']), f"GT2 closed belt {p['belt_length']:.0f} mm (reference)", 'rubber', DARK,
        {'purchased': {'item': f"GT2 closed-loop belt, {p['belt_length']:.0f} mm ({p['belt_length'] / 2:.0f} teeth), 6 mm wide, glass-fibre", 'status': 'to buy'},
         **declared(belt, 0.007 * p['belt_length'] / 1000.0, 'about 7 g/m for 6 mm GT2')})

    # ------------------------------------------------------------ groups, joints
    fixed = [made[n] for n in ('base', 'cap', 'cradle', 'servo', 'horn', 'pulley', 'bearing0', 'bearing1', 'belt')]
    turning = [made[n] for n in ('disc', 'carrier', 'clamp', 'slipring', 'camera_mount', 'camera', 'standoffs', 'board')]
    g_fixed = ops.group(fixed, name='Base (fixed)')
    g_turn = ops.group(turning, name='Turntable (rotating)')
    ops.set_ground(made['base'])
    for nid in fixed[1:]:
        if nid not in (made['pulley'], made['horn'], made['belt']):
            ops.connect_fixed(made['base'], nid)
    for nid in turning[1:]:
        ops.connect_fixed(made['disc'], nid)
    joint = ops.add_joint('continuous', made['base'], made['disc'], (0.0, 0.0, z_disc0), (0.0, 0.0, 1.0), name='Turntable axis')
    ratio = ring_r / pulley_r
    return {'nodes': made, 'groups': {'fixed': g_fixed, 'turning': g_turn}, 'joint': joint,
            'layout': {'center_distance_mm': C, 'ratio': ratio, 'belt_length_mm': path['length'],
                       'driver_wrap_deg': math.degrees(path['wraps'][1]['angle']),
                       'teeth_in_mesh': p['pulley_teeth'] * path['wraps'][1]['angle'] / (2 * math.pi),
                       'z_belt_center_mm': (z_belt0 + z_belt1) / 2, 'z_disc_top_mm': z_disc1, 'camera_height_mm': z_cam}}
