"""Physical-model export in a child process, so the editor stays responsive.

The editor captures a snapshot (`snapshots.capture`), writes its archive as
an `.rcad`, and runs `python -m robocad.export_worker ARCHIVE OUT FLEX PLANAR
SOURCE_FILE PHYSICAL_HASH`. The worker reloads that exact state, derives the
model (signed-distance grids and flexible links are the slow part), writes
OUT atomically and prints a one-line JSON summary on stdout. Failures go to
stderr with a nonzero exit status. The source geometry is never modified.
"""
import json
import os
import sys


def main(argv):
    archive, out, flex, planar, source_file, physical_hash = argv
    from .document import Document
    from .kernel import Plane
    from .physical import export_physical_model

    doc = Document.load(archive)
    model = export_physical_model(doc, None, flex=flex == "1", planar=Plane.xz() if planar == "1" else None)
    # Provenance describes the edited document, not the temporary archive.
    model["source"]["file"] = source_file or None
    model["source"]["physical_hash"] = physical_hash or None
    tmp = f"{out}.{os.getpid()}.tmp"
    with open(tmp, "w") as f:
        json.dump(model, f)
    os.replace(tmp, out)
    print(json.dumps({"links": len(model["links"]), "flexible": sum(1 for l in model["links"] if l.get("flex"))}), flush=True)


def export_snapshot(snapshot, out=None, flex=True, planar=False, source_file=None, timeout=3600):
    """Run the export for a captured snapshot in a child process and return
    the model. Call from a thread other than the GUI's (e.g. a REST handler)."""
    import shutil
    import subprocess
    import tempfile

    folder = tempfile.mkdtemp(prefix="robocad-export-")
    try:
        archive = os.path.join(folder, "model.rcad")
        with open(archive, "wb") as f:
            f.write(snapshot.data)
        target = out or os.path.join(folder, "model.simrobot.json")
        package_parent = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        env = dict(os.environ, PYTHONPATH=os.pathsep.join(filter(None, [package_parent, os.environ.get("PYTHONPATH")])))
        done = subprocess.run([sys.executable, "-m", "robocad.export_worker", archive, target, "1" if flex else "0", "1" if planar else "0",
                               source_file or "", snapshot.physical_hash], capture_output=True, text=True, env=env, timeout=timeout)
        if done.returncode != 0:
            from .kernel import KernelError
            raise KernelError((done.stderr.strip().splitlines() or ["physical export failed"])[-1])
        with open(target) as f:
            return json.load(f)
    finally:
        shutil.rmtree(folder, ignore_errors=True)


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except Exception as e:  # reported to the editor's status bar
        print(f"{type(e).__name__}: {e}", file=sys.stderr, flush=True)
        sys.exit(1)
