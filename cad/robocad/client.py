"""A tiny client for the REST API (stdlib only), also a CLI:

    python -m robocad.client get /doc
    python -m robocad.client op box '{"args": [[0,0,0],[20,10,5]]}'
    python -m robocad.client render out.png --view iso --mode xray
"""

from __future__ import annotations

import json
import sys
import urllib.request
from typing import Any, Optional


class RoboClient:
    def __init__(self, url: str = "http://127.0.0.1:8420"):
        self.url = url.rstrip("/")

    def _req(self, method: str, path: str, body: Optional[dict] = None, raw: bool = False):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(self.url + path, data=data, method=method, headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=180) as r:
                payload = r.read()
        except urllib.error.HTTPError as e:
            payload = e.read()
            try:
                msg = json.loads(payload).get("error", payload.decode())
            except Exception:
                msg = payload.decode(errors="ignore")
            raise RuntimeError(f"{method} {path} → {e.code}: {msg}")
        return payload if raw else json.loads(payload)

    def get(self, path: str):
        return self._req("GET", path)

    def post(self, path: str, body: Optional[dict] = None):
        return self._req("POST", path, body or {})

    def put(self, path: str, body: dict):
        return self._req("PUT", path, body)

    def system_graph(self):
        return self.get('/system')

    def check_system(self, request: dict):
        return self.post('/experiments', {**request, 'preflight': True})

    def experiment_components(self, run_id: str):
        return self.get('/experiments/'+run_id+'/components')

    def experiment_sources(self, run_id: str):
        return self.get('/experiments/'+run_id+'/sources')

    def set_system_graph(self, graph: dict, expected_revision: int):
        return self.put('/system', {'graph': graph, 'expected_revision': expected_revision})

    def add_system_component(self, component: dict, expected_revision: int):
        return self.post('/system/components', {'component': component, 'expected_revision': expected_revision})

    def update_system_component(self, component_id: str, changes: dict, expected_revision: int):
        return self.patch('/system/components/'+component_id, {'component': changes, 'expected_revision': expected_revision})

    def delete_system_component(self, component_id: str, expected_revision: int):
        return self.delete('/system/components/'+component_id+'?expected_revision='+str(expected_revision))

    def connect_system_ports(self, ports: list, expected_revision: int):
        return self.post('/system/connections', {'ports': ports, 'expected_revision': expected_revision})

    def delete_system_connection(self, connection_id: str, expected_revision: int):
        return self.delete('/system/connections/'+connection_id+'?expected_revision='+str(expected_revision))

    def patch(self, path: str, body: dict):
        return self._req("PATCH", path, body)

    def delete(self, path: str):
        return self._req("DELETE", path)

    def op(self, operation: str, *args, **kwargs) -> Any:
        return self.post(f"/ops/{operation}", {"args": list(args), "kwargs": kwargs})["result"]

    def create(self, **spec) -> dict:
        return self.post("/nodes", spec)

    def render(self, path: str, **q) -> str:
        query = "&".join(f"{k}={v}" for k, v in q.items())
        data = self._req("GET", "/render" + (f"?{query}" if query else ""), raw=True)
        with open(path, "wb") as f:
            f.write(data)
        return path

    def screenshot(self, path: str) -> str:
        data = self._req("GET", "/screenshot", raw=True)
        with open(path, "wb") as f:
            f.write(data)
        return path

    # ---- physical model ------------------------------------------------
    def physical(self, path: Optional[str] = None, flex: bool = True) -> dict:
        """The v3 physical assembly description (written to `path` on the server side when given)."""
        return self.get(f"/physical?flex={'1' if flex else '0'}" + (f"&path={path}" if path else ""))

    def results(self) -> dict:
        return self.get("/results")

    def load_results(self, path: str) -> dict:
        return self.post("/results/load", {"path": path})

    def apply_identification(self, path: str) -> dict:
        return self.post("/identification/apply", {"path": path})

    def add_sensor(self, kind: str, body: str, point, name: Optional[str] = None, joint: Optional[str] = None, **opts) -> str:
        return self.post("/sensors", {"kind": kind, "body": body, "point": list(point), "name": name, "joint": joint, **opts})["id"]

    def add_cable(self, from_body: str, from_point, to_body: str, to_point, **opts) -> str:
        return self.post("/cables", {"from_body": from_body, "from_point": list(from_point), "to_body": to_body, "to_point": list(to_point), **opts})["id"]

    def set_battery(self, **spec) -> dict:
        return self.put("/battery", spec)

    def set_control(self, **spec) -> dict:
        return self.put("/control", spec)

    def set_uncertainty(self, **sigmas) -> dict:
        return self.put("/uncertainty", sigmas)

    def threads(self, node_id=None, status=None):
        from urllib.parse import urlencode
        q = urlencode({k: v for k, v in {"node_id": node_id, "status": status}.items() if v is not None})
        return self.get("/threads" + ("?" + q if q else ""))

    def create_thread(self, node_id, point, body, author="Codex", **options):
        return self.post("/threads", {"node_id": node_id, "point": list(point), "body": body, "author": author, **options})

    def reply(self, thread_id, body, author="Codex"):
        return self.post(f"/threads/{thread_id}/comments", {"body": body, "author": author})

    def update_thread(self, thread_id, **changes):
        return self.patch(f"/threads/{thread_id}", changes)

    def edit_comment(self, comment_id, body):
        return self.patch(f"/comments/{comment_id}", {"body": body})

    def delete_comment(self, comment_id):
        return self.delete(f"/comments/{comment_id}")

    def delete_thread(self, thread_id):
        return self.delete(f"/threads/{thread_id}")

    def undo(self):
        return self.post("/undo")

    def redo(self):
        return self.post("/redo")


def main(argv=None):
    argv = argv if argv is not None else sys.argv[1:]
    if not argv:
        print(__doc__)
        return 2
    url = "http://127.0.0.1:8420"
    if argv[0].startswith("http"):
        url, argv = argv[0], argv[1:]
    c = RoboClient(url)
    cmd = argv[0]
    if cmd == "get":
        print(json.dumps(c.get(argv[1]), indent=1))
    elif cmd in ("post", "put", "patch"):
        print(json.dumps(getattr(c, cmd)(argv[1], json.loads(argv[2]) if len(argv) > 2 else {}), indent=1))
    elif cmd == "delete":
        print(json.dumps(c.delete(argv[1]), indent=1))
    elif cmd == "op":
        body = json.loads(argv[2]) if len(argv) > 2 else {}
        print(json.dumps(c.op(argv[1], *body.get("args", []), **body.get("kwargs", {})), indent=1))
    elif cmd == "render":
        q = {}
        it = iter(argv[2:])
        for k in it:
            q[k.lstrip("-")] = next(it)
        print(c.render(argv[1], **q))
    elif cmd == "screenshot":
        print(c.screenshot(argv[1]))
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
