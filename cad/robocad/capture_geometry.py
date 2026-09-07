"""Read-only B-rep inspection of rigid poses captured by the Rust runtime.

This is CAD-side validation, not a second dynamics or collision runtime.
"""
import hashlib
import json
import math
from pathlib import Path
import zipfile

import numpy as np

from .kernel import Body, default_kernel


def captured_cad_transform(com_m, pose):
    """Map original CAD world millimetres to captured world millimetres."""
    from OCP.gp import gp_Trsf

    rotation = np.asarray(pose["rotation"], dtype=float)
    position = np.asarray(pose["position_m"], dtype=float)
    com = np.asarray(com_m, dtype=float)
    if (rotation.shape != (3, 3) or position.shape != (3,) or com.shape != (3,)
            or not all(np.isfinite(x).all() for x in (rotation, position, com))
            or not np.allclose(rotation.T @ rotation, np.eye(3), atol=1e-8, rtol=0)
            or abs(np.linalg.det(rotation) - 1) > 1e-8):
        raise ValueError("finite rigid pose and original COM required")
    matrix = np.column_stack((rotation, (position - rotation @ com) * 1000))
    transform = gp_Trsf()
    transform.SetValues(*matrix.flat)
    return transform


def sdf_cell_probes(sdf, pose, world_point):
    """Expose the eight exported grid nodes surrounding an interior query.

    Inspection only: the Rust sampler remains the collision execution path.
    Weights allow comparison of stored distances with exact CAD distances.
    """
    captured_cad_transform([0, 0, 0], pose)
    rotation = np.asarray(pose["rotation"])
    point = np.asarray(world_point, dtype=float)
    origin = np.asarray(sdf["origin"], dtype=float)
    dims = np.asarray(sdf["dims"])
    cell = sdf["cell"]
    if (point.shape != (3,) or origin.shape != (3,) or dims.shape != (3,)
            or not np.isfinite(point).all() or not np.isfinite(origin).all()
            or not math.isfinite(cell) or cell <= 0 or (dims < 2).any()
            or not np.issubdtype(dims.dtype, np.integer)
            or len(sdf["values"]) != math.prod(int(d) for d in dims)):
        raise ValueError("finite point and complete distance grid required")
    local = rotation.T @ (point - pose["position_m"])
    grid = (local - origin) / cell
    if (grid < 0).any() or (grid >= dims - 1).any():
        raise ValueError("cell inspection requires an interior grid query")
    base = np.floor(grid).astype(int)
    fraction = grid - base
    rows = []
    for offset in np.ndindex(2, 2, 2):
        index = base + offset
        value = sdf["values"][(index[0] * dims[1] + index[1]) * dims[2] + index[2]]
        if not math.isfinite(value):
            raise ValueError("finite grid distances required")
        rows.append({"index": index.tolist(), "stored_distance_m": value,
                     "weight": float(np.prod(np.where(offset, fraction, 1 - fraction))),
                     "point_world_m": (rotation @ (origin + cell * index) + pose["position_m"]).tolist()})
    return rows


def audit_captured_pair(cad_path, scene, capture, left, right, times, probe_frame_s=None, probe_sdf_cells=False,
                        *, measure_pair_distances=True):
    """Inspect exact B-rep member distances and captured contact points.

    Distances are nonnegative: zero cannot distinguish touching from overlap.
    Point classification checks every solid; it is not continuous clearance.
    Set measure_pair_distances=False for focused contact-point inspection without
    expensive whole-shape distance queries. An empty distance list then means
    unmeasured, not separated; the report records this explicitly.
    """
    from OCP.BRepBuilderAPI import BRepBuilderAPI_Transform, BRepBuilderAPI_MakeVertex
    from OCP.BRepClass3d import BRepClass3d_SolidClassifier
    from OCP.TopAbs import TopAbs_SOLID, TopAbs_IN, TopAbs_ON, TopAbs_OUT
    from OCP.TopExp import TopExp_Explorer
    from OCP.TopAbs import TopAbs_FACE
    from OCP.TopoDS import TopoDS_Compound
    from OCP.BRep import BRep_Builder
    from OCP.gp import gp_Pnt

    cad_path = Path(cad_path)
    digest = hashlib.sha256(cad_path.read_bytes()).hexdigest()
    source = scene["robot"]["source"]
    if (source.get("cad_sha256") != digest or capture.get("source") != source
            or capture.get("completed") is not True or capture.get("error") is not None):
        raise ValueError("complete capture and exact CAD provenance required")
    if left == right or not times or any(not math.isfinite(t) or t < 0 for t in times):
        raise ValueError("distinct links and explicit finite nonnegative sample times required")
    links = scene["robot"]["links"]
    chosen = []
    for name in (left, right):
        matches = [(i, link) for i, link in enumerate(links) if link["name"] == name]
        if len(matches) != 1:
            raise ValueError(f"unique exported link required: {name}")
        chosen.append(matches[0])
    kernel = default_kernel()
    bodies = {}
    with zipfile.ZipFile(cad_path) as archive:
        manifest = json.loads(archive.read("manifest.json"))
        nodes = {n["id"]: n for n in manifest["nodes"]}
        for _, link in chosen:
            members = link.get("members", [])
            if not members or len(set(members)) != len(members):
                raise ValueError("explicit unique CAD link members required")
            for member in members:
                node = nodes[member]
                bodies[member] = kernel.deserialize(archive.read(f"brep/{member}.brep"), node.get("body_kind", "solid"))
    frames = capture["frames"]
    def exact_frame(time):
        matching = [f for f in frames if math.isfinite(f["time_s"]) and abs(f["time_s"] - time) <= 1e-10]
        if len(matching) != 1:
            raise ValueError(f"exactly one captured frame required at {time}")
        return matching[0]

    def pose_for(frame, index):
        poses = [p for p in frame["poses"] if p["name"] == links[index]["name"]]
        if len(poses) != 1:
            raise ValueError("unique captured link pose required")
        captured_cad_transform(links[index]["com"], poses[0])
        return poses[0]

    def pair_contacts(frame):
        return [c for c in frame.get("contacts", frame.get("internal_contacts", []))
                if {c["link"], c.get("other")} == {chosen[0][0], chosen[1][0]}]

    seeds = []
    if probe_frame_s is not None:
        if not math.isfinite(probe_frame_s) or probe_frame_s < 0:
            raise ValueError("finite nonnegative probe frame required")
        seed_frame = exact_frame(probe_frame_s)
        for contact in pair_contacts(seed_frame):
            pose = pose_for(seed_frame, contact["link"])
            point = np.asarray(contact["point_m"], dtype=float)
            if point.shape != (3,) or not np.isfinite(point).all():
                raise ValueError("finite contact seed required")
            local = np.asarray(pose["rotation"]).T @ (point - pose["position_m"])
            seeds.append((contact, local))
        if not seeds:
            raise ValueError("probe frame must contain a contact for this pair")
    rows = []
    for time in times:
        frame = exact_frame(time)
        moved = {}
        surfaces = {}
        for _, link in chosen:
            poses = [p for p in frame["poses"] if p["name"] == link["name"]]
            if len(poses) != 1:
                raise ValueError("unique captured link pose required")
            transform = captured_cad_transform(link["com"], poses[0])
            for member in link["members"]:
                moved[member] = Body(BRepBuilderAPI_Transform(bodies[member].shape, transform, True).Shape())
                # Solid distance is zero for an interior point. Query its faces
                # separately when diagnosing the magnitude of a signed field.
                boundary = TopoDS_Compound()
                builder = BRep_Builder()
                builder.MakeCompound(boundary)
                faces = TopExp_Explorer(moved[member].shape, TopAbs_FACE)
                while faces.More():
                    builder.Add(boundary, faces.Current())
                    faces.Next()
                surfaces[member] = Body(boundary)
        distances = []
        if measure_pair_distances:
            for a in chosen[0][1]["members"]:
                for b in chosen[1][1]["members"]:
                    gap, pa, pb = kernel.distance(moved[a], moved[b])
                    distances.append({"a": a, "b": b, "a_name": nodes[a]["name"], "b_name": nodes[b]["name"],
                                      "distance_mm": gap, "a_point_world_mm": pa, "b_point_world_mm": pb})
        probes = []
        probes_at_time = [(c, c["point_m"], None, None) for c in pair_contacts(frame)]
        if probe_sdf_cells:
            for contact in pair_contacts(frame):
                target = links[contact["other"]]
                sdf = target.get("collision", {}).get("sdf")
                if sdf is None:
                    raise ValueError("exported target distance grid required for cell inspection")
                for cell in sdf_cell_probes(sdf, pose_for(frame, contact["other"]), contact["point_m"]):
                    probes_at_time.append((contact, cell["point_world_m"], None, cell))
        for seed, local in seeds:
            pose = pose_for(frame, seed["link"])
            point = np.asarray(pose["rotation"]) @ local + pose["position_m"]
            probes_at_time.append((seed, point.tolist(), probe_frame_s, None))
        for contact, world_point, seed_time, cell in probes_at_time:
            target = links[contact["other"]]
            xyz = np.asarray(world_point, dtype=float) * 1000
            if xyz.shape != (3,) or not np.isfinite(xyz).all():
                raise ValueError("finite captured contact point required")
            point = gp_Pnt(*xyz)
            vertex = Body(BRepBuilderAPI_MakeVertex(point).Vertex())
            members = []
            for member in target["members"]:
                states = []
                explorer = TopExp_Explorer(moved[member].shape, TopAbs_SOLID)
                while explorer.More():
                    state = BRepClass3d_SolidClassifier(explorer.Current(), point, 1e-6).State()
                    states.append({TopAbs_IN: "inside", TopAbs_ON: "boundary", TopAbs_OUT: "outside"}.get(state, "unknown"))
                    explorer.Next()
                gap, _, nearest = kernel.distance(vertex, moved[member])
                surface_gap, _, _ = kernel.distance(vertex, surfaces[member])
                members.append({"member": member, "name": nodes[member]["name"], "solid_states": states,
                                "distance_mm": gap, "surface_distance_mm": surface_gap, "nearest_world_mm": nearest})
            probes.append({"runtime_contact": contact if seed_time is None and cell is None else None,
                           "seed_frame_s": seed_time, "point_world_m": world_point,
                           "target": target["name"], "brep_members": members})
            if cell is not None:
                probes[-1]["sdf_cell_node"] = cell
        rows.append({"time_s": time, "member_distances": distances, "contact_point_probes": probes})
    return {"cad_sha256": digest, "links": [left, right], "frames": rows,
            "pair_distances_measured": measure_pair_distances,
            "scope": "Read-only exact B-rep distance and solid classification at captured rigid poses. CAD/world millimetres; runtime inputs metres. Nonnegative shape distance alone cannot distinguish touching from overlap. Point classification uses 1e-6 mm tolerance. No continuous clearance or hardware certification."}
