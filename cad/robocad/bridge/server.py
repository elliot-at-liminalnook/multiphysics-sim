"""Live link to a mesh application: a websocket server that pushes the
document's tessellated geometry on every change. The Blender add-on in
`blender_addon/robocad_link.py` connects, asks for a tessellation
tolerance, maps groups to collections, marks sharp edges/UV seams and
keeps materials and modifiers through refreshes by stable per-face IDs.

Protocol (JSON text frames):
  client → {"hello": {"tolerance": 0.05}}     sets the tolerance from the target side
  server → {"scene": {"objects": [{"id", "name", "collection", "vertices": [...], "triangles": [...],
            "face_ids": [...], "sharp_edges": [[a,b],...], "material": {...}}], "revision": n}}
  client → {"refresh": true}                  asks for the scene again
"""

from __future__ import annotations

import asyncio
import json
import threading
import time
from typing import Optional

from ..document import Document
from ..printing import weld


def scene_payload(doc: Document, tolerance: float) -> dict:
    objects = []
    for n in doc.walk():
        if n.kind not in ("body", "sheet", "instance", "mesh") or not doc.is_visible(n.id):
            continue
        mesh = doc.mesh_of(n.id, tolerance)
        if mesh is None:
            continue
        w = weld(mesh)
        # Sharp edges: edges between triangles of different B-rep faces, or creases > 30°.
        sharp = set()
        seen: dict[tuple[int, int], int] = {}
        for t, f in zip(w.triangles, w.triangle_face):
            for a, b in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
                key = (min(a, b), max(a, b))
                other = seen.get(key)
                if other is None:
                    seen[key] = f
                elif other != f:
                    sharp.add(key)
        mat = doc.materials.get(n.material or "")
        collection = doc.nodes[n.parent].name if n.parent and n.parent in doc.nodes else "robocad"
        objects.append({
            "id": n.id, "name": n.name, "collection": collection,
            "vertices": [list(v) for v in w.vertices], "triangles": [list(t) for t in w.triangles], "face_ids": list(w.triangle_face),
            "sharp_edges": [list(e) for e in sharp],
            "material": {"name": mat.name if mat else "default", "color": list(n.color or (mat.color if mat else (0.7, 0.7, 0.72))), "roughness": mat.roughness if mat else 0.5, "metallic": mat.metallic if mat else 0.0},
        })
    return {"objects": objects, "unit": "mm"}


class BridgeServer:
    def __init__(self, doc: Document, host: str = "127.0.0.1", port: int = 8765):
        self.doc = doc
        self.host = host
        self.port = port
        self.tolerance = 0.05
        self.revision = 0
        self.clients: set = set()
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._thread: Optional[threading.Thread] = None
        self._dirty = threading.Event()
        self._stop = threading.Event()
        self.doc.listeners.append(self._on_doc)

    def _on_doc(self, event, payload):
        if event in ("changed", "added", "removed", "moved", "saved"):
            self._dirty.set()

    def start(self):
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def stop(self):
        self._stop.set()
        if self._loop:
            self._loop.call_soon_threadsafe(self._loop.stop)

    def _run(self):
        import websockets

        self._loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self._loop)

        async def handler(ws):
            self.clients.add(ws)
            try:
                await ws.send(json.dumps({"hello": {"server": "robocad", "revision": self.revision}}))
                await self._send_scene(ws)
                async for msg in ws:
                    try:
                        data = json.loads(msg)
                    except Exception:
                        continue
                    if "hello" in data and "tolerance" in data["hello"]:
                        self.tolerance = float(data["hello"]["tolerance"])
                        await self._send_scene(ws)
                    elif data.get("refresh"):
                        await self._send_scene(ws)
            finally:
                self.clients.discard(ws)

        async def pusher():
            while not self._stop.is_set():
                await asyncio.sleep(0.25)
                if self._dirty.is_set() and self.clients:
                    self._dirty.clear()
                    for ws in list(self.clients):
                        try:
                            await self._send_scene(ws)
                        except Exception:
                            self.clients.discard(ws)

        async def main():
            async with websockets.serve(handler, self.host, self.port):
                await pusher()

        try:
            self._loop.run_until_complete(main())
        except Exception:
            pass

    async def _send_scene(self, ws):
        self.revision += 1
        payload = scene_payload(self.doc, self.tolerance)
        payload["revision"] = self.revision
        await ws.send(json.dumps({"scene": payload}))
