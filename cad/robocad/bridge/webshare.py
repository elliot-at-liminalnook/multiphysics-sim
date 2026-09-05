"""Web share: a single static HTML viewer with the tessellated geometry
embedded (three.js from a CDN). Nothing editable leaves the machine."""

from __future__ import annotations

import base64
import json
import struct

from ..document import Document
from ..printing import weld


def publish(doc: Document, path: str, tolerance: float = 0.05) -> str:
    objects = []
    for n in doc.walk():
        if n.kind not in ("body", "sheet", "instance", "mesh") or not doc.is_visible(n.id):
            continue
        m = doc.mesh_of(n.id, tolerance)
        if m is None:
            continue
        w = weld(m)
        verts = struct.pack(f"<{3*len(w.vertices)}f", *[c for v in w.vertices for c in v])
        tris = struct.pack(f"<{3*len(w.triangles)}I", *[i for t in w.triangles for i in t])
        mat = doc.materials.get(n.material or "")
        objects.append({"name": n.name, "color": list(n.color or (mat.color if mat else (0.7, 0.7, 0.72))), "v": base64.b64encode(verts).decode(), "t": base64.b64encode(tris).decode()})
    html = f"""<!doctype html><html><head><meta charset="utf-8"><title>{(doc.path or 'robocad').split('/')[-1]} — robocad viewer</title>
<style>html,body{{margin:0;height:100%;background:#1c1e22;color:#ddd;font-family:sans-serif}}#info{{position:absolute;left:10px;top:8px;font-size:13px}}</style></head>
<body><div id="info">drag to orbit • wheel to zoom • right-drag to pan</div>
<script type="importmap">{{"imports":{{"three":"https://cdn.jsdelivr.net/npm/three@0.160.0/build/three.module.js","three/addons/":"https://cdn.jsdelivr.net/npm/three@0.160.0/examples/jsm/"}}}}</script>
<script type="module">
import * as THREE from 'three';
import {{ OrbitControls }} from 'three/addons/controls/OrbitControls.js';
const objects = {json.dumps(objects)};
const scene = new THREE.Scene(); scene.background = new THREE.Color(0x1c1e22);
const camera = new THREE.PerspectiveCamera(40, innerWidth/innerHeight, 0.1, 100000);
const renderer = new THREE.WebGLRenderer({{antialias:true}}); renderer.setSize(innerWidth, innerHeight); document.body.appendChild(renderer.domElement);
scene.add(new THREE.HemisphereLight(0xffffff, 0x444466, 1.0)); const d = new THREE.DirectionalLight(0xffffff, 1.2); d.position.set(1,-2,3); scene.add(d);
const box = new THREE.Box3();
function decode(b64, ctor) {{ const bin = atob(b64); const buf = new ArrayBuffer(bin.length); const u8 = new Uint8Array(buf); for (let i=0;i<bin.length;i++) u8[i]=bin.charCodeAt(i); return new ctor(buf); }}
for (const o of objects) {{ const g = new THREE.BufferGeometry(); g.setAttribute('position', new THREE.BufferAttribute(decode(o.v, Float32Array), 3)); g.setIndex(new THREE.BufferAttribute(decode(o.t, Uint32Array), 1)); g.computeVertexNormals();
 const m = new THREE.Mesh(g, new THREE.MeshStandardMaterial({{color: new THREE.Color(...o.color), roughness: 0.6}})); m.name = o.name; scene.add(m); box.expandByObject(m); }}
scene.up = new THREE.Vector3(0,0,1); camera.up.set(0,0,1);
const c = box.getCenter(new THREE.Vector3()); const s = box.getSize(new THREE.Vector3()).length() || 100;
camera.position.set(c.x + s, c.y - s, c.z + s*0.8); const controls = new OrbitControls(camera, renderer.domElement); controls.target.copy(c);
const grid = new THREE.GridHelper(s*2, 20, 0x555555, 0x333333); grid.rotation.x = Math.PI/2; grid.position.z = box.min.z; scene.add(grid);
addEventListener('resize', () => {{ camera.aspect = innerWidth/innerHeight; camera.updateProjectionMatrix(); renderer.setSize(innerWidth, innerHeight); }});
(function loop() {{ requestAnimationFrame(loop); controls.update(); renderer.render(scene, camera); }})();
</script></body></html>"""
    with open(path, "w") as f:
        f.write(html)
    return html
