bl_info = {"name": "robocad live link", "author": "robocad", "version": (0, 1, 0), "blender": (3, 0, 0), "location": "View3D > Sidebar > robocad", "description": "Receives tessellated geometry from robocad over a websocket; groups become collections, sharp edges and UV seams are marked, materials and modifiers survive refreshes through stable face IDs.", "category": "Import-Export"}

import json
import socket
import struct
import threading
import base64
import os

import bpy

_state = {"thread": None, "sock": None, "queue": [], "running": False, "tolerance": 0.05}


# A tiny RFC6455 client (no external dependency inside Blender).
def _ws_connect(host, port):
    s = socket.create_connection((host, port))
    key = base64.b64encode(os.urandom(16)).decode()
    s.sendall(f"GET / HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n".encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += s.recv(4096)
    return s


def _ws_send(s, text):
    data = text.encode()
    mask = os.urandom(4)
    header = bytearray([0x81])
    n = len(data)
    if n < 126:
        header.append(0x80 | n)
    elif n < 65536:
        header += bytes([0x80 | 126]) + struct.pack(">H", n)
    else:
        header += bytes([0x80 | 127]) + struct.pack(">Q", n)
    s.sendall(bytes(header) + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))


def _ws_recv(s):
    def read(n):
        out = b""
        while len(out) < n:
            chunk = s.recv(n - len(out))
            if not chunk:
                raise ConnectionError
            out += chunk
        return out

    b0, b1 = read(2)
    n = b1 & 0x7F
    if n == 126:
        n = struct.unpack(">H", read(2))[0]
    elif n == 127:
        n = struct.unpack(">Q", read(8))[0]
    mask = read(4) if b1 & 0x80 else None
    data = read(n)
    if mask:
        data = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
    return data.decode(errors="ignore") if (b0 & 0x0F) == 1 else None


def _reader(host, port):
    try:
        s = _ws_connect(host, port)
        _state["sock"] = s
        _ws_send(s, json.dumps({"hello": {"tolerance": _state["tolerance"]}}))
        while _state["running"]:
            msg = _ws_recv(s)
            if msg:
                _state["queue"].append(msg)
    except Exception as e:
        _state["queue"].append(json.dumps({"error": str(e)}))
    _state["running"] = False


def _apply_scene(scene):
    for o in scene["objects"]:
        coll = bpy.data.collections.get(o["collection"]) or bpy.data.collections.new(o["collection"])
        if coll.name not in bpy.context.scene.collection.children:
            bpy.context.scene.collection.children.link(coll)
        name = f"rc_{o['id']}"
        obj = bpy.data.objects.get(name)
        mesh = bpy.data.meshes.new(o["name"])
        mesh.from_pydata([tuple(v) for v in o["vertices"]], [], [tuple(t) for t in o["triangles"]])
        mesh.update()
        # Stable per-face IDs as an int attribute so materials assigned per face survive.
        attr = mesh.attributes.new("robocad_face", "INT", "FACE")
        for i, fid in enumerate(o["face_ids"]):
            attr.data[i].value = fid
        edge_index = {(min(e.vertices), max(e.vertices)): e for e in mesh.edges}
        for a, b in o["sharp_edges"]:
            e = edge_index.get((min(a, b), max(a, b)))
            if e is not None:
                e.use_edge_sharp = True
                e.use_seam = True
        if obj is None:
            obj = bpy.data.objects.new(o["name"], mesh)
            obj.name = name
            coll.objects.link(obj)
            mat = bpy.data.materials.new(o["material"]["name"])
            mat.use_nodes = True
            bsdf = mat.node_tree.nodes.get("Principled BSDF")
            if bsdf:
                c = o["material"]["color"]
                bsdf.inputs["Base Color"].default_value = (c[0], c[1], c[2], 1.0)
                bsdf.inputs["Roughness"].default_value = o["material"]["roughness"]
                bsdf.inputs["Metallic"].default_value = o["material"]["metallic"]
            mesh.materials.append(mat)
        else:
            old = obj.data
            # Keep the materials and (by name) per-face material indices via face ids.
            face_mat = {}
            if "robocad_face" in old.attributes:
                ids = old.attributes["robocad_face"].data
                for p in old.polygons:
                    face_mat[ids[p.index].value] = p.material_index
            for m in old.materials:
                mesh.materials.append(m)
            for p in mesh.polygons:
                p.material_index = face_mat.get(attr.data[p.index].value, 0)
            obj.data = mesh
            bpy.data.meshes.remove(old)
        obj.scale = (0.001, 0.001, 0.001)  # mm → m
        obj["robocad_id"] = o["id"]


def _timer():
    while _state["queue"]:
        msg = _state["queue"].pop(0)
        try:
            data = json.loads(msg)
        except Exception:
            continue
        if "scene" in data:
            _apply_scene(data["scene"])
    return 0.5 if _state["running"] else None


class ROBOCAD_OT_connect(bpy.types.Operator):
    bl_idname = "robocad.connect"
    bl_label = "Connect to robocad"

    def execute(self, context):
        if _state["running"]:
            return {"CANCELLED"}
        _state["running"] = True
        _state["tolerance"] = context.scene.robocad_tolerance
        _state["thread"] = threading.Thread(target=_reader, args=(context.scene.robocad_host, context.scene.robocad_port), daemon=True)
        _state["thread"].start()
        bpy.app.timers.register(_timer)
        return {"FINISHED"}


class ROBOCAD_OT_disconnect(bpy.types.Operator):
    bl_idname = "robocad.disconnect"
    bl_label = "Disconnect"

    def execute(self, context):
        _state["running"] = False
        try:
            _state["sock"].close()
        except Exception:
            pass
        return {"FINISHED"}


class ROBOCAD_OT_tolerance(bpy.types.Operator):
    bl_idname = "robocad.tolerance"
    bl_label = "Apply tolerance"

    def execute(self, context):
        _state["tolerance"] = context.scene.robocad_tolerance
        if _state["sock"]:
            _ws_send(_state["sock"], json.dumps({"hello": {"tolerance": _state["tolerance"]}}))
        return {"FINISHED"}


class ROBOCAD_PT_panel(bpy.types.Panel):
    bl_label = "robocad live link"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "robocad"

    def draw(self, context):
        l = self.layout
        l.prop(context.scene, "robocad_host")
        l.prop(context.scene, "robocad_port")
        l.prop(context.scene, "robocad_tolerance")
        l.operator("robocad.tolerance")
        l.operator("robocad.connect" if not _state["running"] else "robocad.disconnect")


def register():
    bpy.types.Scene.robocad_host = bpy.props.StringProperty(name="Host", default="127.0.0.1")
    bpy.types.Scene.robocad_port = bpy.props.IntProperty(name="Port", default=8765)
    bpy.types.Scene.robocad_tolerance = bpy.props.FloatProperty(name="Tessellation tolerance (mm)", default=0.05, min=0.001, max=2.0)
    for c in (ROBOCAD_OT_connect, ROBOCAD_OT_disconnect, ROBOCAD_OT_tolerance, ROBOCAD_PT_panel):
        bpy.utils.register_class(c)


def unregister():
    for c in (ROBOCAD_OT_connect, ROBOCAD_OT_disconnect, ROBOCAD_OT_tolerance, ROBOCAD_PT_panel):
        bpy.utils.unregister_class(c)
