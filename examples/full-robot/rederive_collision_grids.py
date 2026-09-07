"""Reproduce an explicit CAD-derived distance-grid correction off the UI thread.

Only named grids and derivation provenance change. Existing dynamics parameters,
mesh samples, grid cell sizes and collision exclusions are preserved.
"""
import argparse
import hashlib
import json
from pathlib import Path
import time
import zipfile

import numpy as np

from robocad.kernel import default_kernel
from robocad.physical import _weld_np, signed_distance_grid, solid_collision_meshes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("recipe", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    recipe = json.loads(args.recipe.read_text())
    digest = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
    for key in ("cad", "scene"):
        if digest(recipe[key]) != recipe[key + "_sha256"]:
            raise ValueError(f"{key} hash mismatch")
    if args.output.resolve() in (Path(recipe["scene"]).resolve(), Path(recipe["cad"]).resolve()):
        raise ValueError("use a new output path")
    scene = json.loads(Path(recipe["scene"]).read_text())
    if scene["robot"]["source"]["cad_sha256"] != recipe["cad_sha256"]:
        raise ValueError("scene/CAD provenance mismatch")
    names = recipe["links"]
    if not names or len(set(names)) != len(names):
        raise ValueError("explicit unique link names required")
    if recipe["algorithm"] != "triangle_aabb_exact_v1" or recipe["tolerance_mm"] != .15:
        raise ValueError("recipe must match shared exporter derivation")
    if recipe.get("sign_algorithm") not in (None, "solid_union_ray_v1"):
        raise ValueError("unsupported sign derivation")
    kernel = default_kernel()
    reports = []
    with zipfile.ZipFile(recipe["cad"]) as archive:
        manifest = json.loads(archive.read("manifest.json"))
        nodes = {n["id"]: n for n in manifest["nodes"]}
        for name in names:
            matches = [l for l in scene["robot"]["links"] if l["name"] == name]
            if len(matches) != 1:
                raise ValueError(f"unique link required: {name}")
            link = matches[0]
            meshes = []
            solid_meshes = [] if recipe.get("sign_algorithm") else None
            started = time.monotonic()
            for member in link["members"]:
                body = kernel.deserialize(archive.read(f"brep/{member}.brep"), nodes[member].get("body_kind", "solid"))
                mesh = kernel.tessellate(body, recipe["tolerance_mm"])
                if mesh.vertices:
                    v, t = _weld_np(mesh)
                    meshes.append((v * .001 - link["com"], t))
                if solid_meshes is not None:
                    solid_meshes.extend(solid_collision_meshes(kernel, body, np.asarray(link['com'])))
            old = link["collision"]["sdf"]
            new = signed_distance_grid(meshes, old["cell"], solid_meshes=solid_meshes)
            if old["dims"] != new["dims"] or not np.allclose(old["origin"], new["origin"], atol=1e-10, rtol=0):
                raise ValueError(f"grid domain changed for {name}; investigate tessellation identity")
            link["collision"]["sdf"] = new
            if solid_meshes is not None:
                link['collision']['sign_derivation'] = {'algorithm': 'solid_union_ray_v1',
                    'solid_count': len(solid_meshes), 'non_solid_surfaces': 'unsigned_distance_only'}
            report = {"link": name, "wall_s": time.monotonic() - started,
                      "maximum_node_change_m": float(np.max(np.abs(np.asarray(new["values"]) - old["values"]))),
                      "cell_m": old["cell"], "dims": old["dims"]}
            reports.append(report)
            print(json.dumps(report), flush=True)
    scene["robot"]["source"]["collision_distance_experiment"] = {
        "recipe": recipe, "recipe_sha256": digest(args.recipe),
        "exporter_sha256": digest("cad/robocad/physical.py"),
        "scope": "Named grids regenerated from CAD; other grids retain the input derivation. No clearance or walking certification."}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(scene))
    args.output.with_suffix(".derivation.json").write_text(json.dumps({"output_sha256": digest(args.output), "links": reports}, indent=2))


if __name__ == "__main__":
    main()
