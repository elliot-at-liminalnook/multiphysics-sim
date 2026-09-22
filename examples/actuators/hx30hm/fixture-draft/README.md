# HX-30HM modular fixture — draft 01

A PLA housing clamp for a Bambu H2C. Print one module per motor, up to twelve. Each base mounts independently to the chosen work surface. This is an initial fit/design review, **not a strength-qualified stress-test fixture**.

## Review and files

- `index.html`: orbitable CAD preview, one/four/twelve motors, removable cap.
- `print-kit.3mf`: one cradle, cap, optional row link and fit coupon, separated and flat on the plate. Geometry only; select your H2C and filament in Bambu Studio.
- `cradle.stl`, `cap.stl`, `row-link.stl`, `fit-coupon.stl`: individual printable parts; STL coordinates in mm.
- `print-kit.rcad`, `assembly.rcad`: editable native CAD, with material and uncertainty metadata. Motor/hardware are reference geometry in the assembly only.
- `print-kit.step`: printable parts as editable solids.
- `assembled.png`, `exploded.png`, `print-layout.png`: quick review images.
- `validation.json`: actual geometry checks, volumes, source hash. Solid-material mass is not a slicer filament estimate.

## Mechanical layout

100 × 86 × 8 mm base. The motor lies on its broad side, on two 4 mm raised contact pads. Side fences provide a case contact path for torque. A 7 mm cap has an open window and four M4 through-bolts into underside nut pockets; the cap floats above the fences and contacts the case. Do not torque the cap down to the fences. Its slot adjustment is 2 mm along the shaft direction.

The assumed housing cavity is 46.0 mm wide: nominal 45.2 mm plus 0.4 mm per side. The fixture does not use estimated mounting-ear holes. Housing screws, seams, connector protrusions and dimensional tolerances still need checking. Shim the lateral clearance if needed; no calibrated friction or tightening torque is assumed.

The output shaft points out beyond the front base edge, so a lever/pulley can run in a plane outside the plate. The opposite end is open for the rear boss and bus connectors. The illustrative shaft position is unmeasured. The cap relies on housing friction for axial retention; an axial pull test needs a positive end retainer in the next revision. Large pulleys/levers and bench-edge placement need checking against the actual moving envelope.

Four 6.6 mm wide, 16.6 mm overall mounting slots accept M5/M6 through-bolts with washers, or suitable screws for the actual substrate. Flat rear pads allow metal bench clamps. Curved/irregular surfaces need a rigid flat backing plate; this base alone does not conform to pipes. Cable ties are for cable strain relief only.

Optional row links attach at X = ±44 mm, Y = 0. With bases on 110 mm centers, one 40 × 16 × 6 mm link spans two 4.5 mm holes 22 mm apart. Links align modules; **each module still needs its own surface attachment**. The browser uses larger 150 × 165 mm spacing, without links; choose final spacing from the load attachments. Twelve modules are multiple separate prints, not one huge base.

## Starting print and hardware assumptions

Proposed starting settings, not sliced/verified: PLA, 0.20 mm layers, six walls, 50% gyroid, six top/bottom layers. Print the base underside and cap broad face on the bed as laid out. No designed overhang requires generated supports; inspect the small nut-pocket roof bridges in the slicer. Use the installed PLA profile's temperatures and chamber/ventilation settings. Colors are explanatory; the 3MF is intended for one PLA filament.

Per motor:

- 1 cradle + 1 cap.
- 4 M4 × 45 mm socket screws, 4 M4 hex nuts (nominal 7 mm across flats), and washers totaling about 2 mm under each head. The 7.5 mm AF nut pockets are 4.6 mm deep. Check nut capture and thread engagement; bolt ends must not protrude below the base. Nut-pocket clearance is provisional; verify it captures your nuts without spinning.
- Four surface fasteners and washers, **or** suitable independent metal clamps. Fastener length/type depends on the mounting surface; not included in the printed kit.
- Optional row link: 2 M4 through-bolts, nuts and washers; underside nuts require clearance in the backing surface.

For twelve motors: 12 cradles, 12 caps, 48 clamp screws/nuts, and 12 independent surface attachments. Print the small coupon first to check the 46 mm housing opening, then one complete module to check cap, screw and connector access.

PLA clamp preload, creep, case heating, layer adhesion and actual mounting-surface strength are uncalibrated. Do not treat the servo's published stall torque as a proven fixture rating. A physical fit check and a defined load direction/lever arm are needed before qualifying this for powered stress tests.

## Provenance and reproduction

Geometry uses the shared `robocad` Open CASCADE kernel, material library, validation and exporters. Rebuild from the repository root:

```sh
cad/.venv/bin/python cad/scripts/hx30hm_fixture.py
```

Nominal 45.2 × 24.7 × 35 mm envelope comes from the existing `MOTOR_LIBRARY['hx30hm']`, corroborated against the local Hiwonder manual (Parameter table) and [manufacturer page](https://www.hiwonder.com/products/hx-30hm). The orientation and shaft offset in the preview are estimates. The existing actuator library is unchanged.

All parts are substantially inside the H2C's published single-nozzle 305 × 320 × 325 mm envelope ([Bambu specifications](https://blog.bambulab.com/bambu-lab-h2c-where-multi-material-vortek-system-meets-engineering-precision/)). No printer-specific machine configuration or G-code is included.

Validated: solid validity, positive volume, closed meshes, separate print-layout bounding boxes. Not validated: actual print, fit, preload, structural/thermal response, dynamic clearance or safe load rating. No physics simulation was run for this draft.
