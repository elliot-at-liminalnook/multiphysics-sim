"""Cheap fresh filesystem measurement injected into every agent turn."""
import json
import shutil
import time

def snapshot(workspace):
    usage = shutil.disk_usage(workspace)
    return {"measured_at_unix": time.time(), "workspace_filesystem": str(workspace),
            "free_gib": round(usage.free / 1024**3, 2),
            "total_gib": round(usage.total / 1024**3, 2),
            "build_planning_baseline_gib": 20,
            "emergency_floor_gib": 2,
            "note": "Fresh measurement, not a guarantee a build fits. Assess anticipated growth; remeasure before deletion or building. No files were deleted by this preflight."}

def context(workspace):
    return "\n\nDISK-SPACE PREFLIGHT (fresh for this turn):\n" + json.dumps(snapshot(workspace)) + "\nIf this is short for the planned builds, free regenerable build output first (see your role prompt). The coordinator pauses the run below the emergency floor."
