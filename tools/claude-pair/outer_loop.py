"""Product direction above the bounded orchestrator/worker execution loop."""
import copy
import json
import time


def obj(properties):
    return {"type": "object", "properties": properties,
            "required": list(properties), "additionalProperties": False}


TEXT = {"type": "string"}
STRINGS = {"type": "array", "items": TEXT}
CATEGORY = {"enum": ["cohesion", "feature_gap", "library", "tech_debt"]}
TASK = obj({"id": TEXT, "title": TEXT, "brief": TEXT,
            "done_when": STRINGS, "checks": STRINGS})
BATCH = obj({"id": TEXT, "title": TEXT, "objective": TEXT,
             "outcomes": STRINGS, "out_of_scope": STRINGS,
             "tasks": {"type": "array", "items": TASK}})
DIRECTOR_SCHEMA = obj({
    "coordination_notes": STRINGS, "action": {"enum": ["select", "stop"]}, "summary": TEXT,
    "rationale": TEXT, "selected_id": TEXT,
    "candidates": {"type": "array", "items": obj({
        "id": TEXT, "title": TEXT, "category": CATEGORY, "problem": TEXT,
        "evidence": STRINGS, "benefit": TEXT, "leverage": TEXT,
        "effort": {"enum": ["small", "medium", "large"]},
        "risk": TEXT, "disposition": {"enum": ["select", "defer", "reject"]},
        "reason": TEXT})},
    "batch": BATCH})


def check_passed(receipt, waived=()):
    """A failure is waivable only when the orchestrator names it and the same
    command was already failing before the assignment's edits began."""
    if receipt["exit_code"] == 0:
        return True
    before = receipt.get("before") or {}
    return receipt["name"] in waived and before.get("exit_code") not in (None, 0)


def nonempty(values):
    return bool(values) and all(isinstance(v, str) and v.strip() for v in values)


def guard_decision(decision, state, checks):
    candidates = decision["candidates"]
    ids = [c["id"] for c in candidates]
    if not nonempty([decision["summary"], decision["rationale"]]):
        raise ValueError("Director must explain its decision")
    if len(ids) != len(set(ids)) or any(not i.strip() for i in ids):
        raise ValueError("Hopper candidate IDs must be nonempty and unique")
    for c in candidates:
        if not nonempty(c["evidence"]) or not nonempty([c["problem"], c["benefit"], c["reason"], c["leverage"], c["risk"]]):
            raise ValueError("Every candidate needs source evidence and tradeoffs")
    picked = [c["id"] for c in candidates if c["disposition"] == "select"]
    batch = decision["batch"]
    if decision["action"] == "stop":
        if picked or decision["selected_id"] or batch["tasks"]:
            raise ValueError("A stop decision cannot dispatch work")
        return
    if not 3 <= len(candidates) <= 6 or len({c["category"] for c in candidates}) < 2:
        raise ValueError("Compare 3–6 candidates across at least two categories")
    if picked != [decision["selected_id"]] or batch["id"] != decision["selected_id"]:
        raise ValueError("Select exactly one candidate and matching batch")
    history = state["outer"]["history"]
    if batch["id"] in {b["id"] for b in history}:
        raise ValueError("Cannot repeat an already completed batch ID")
    if not 1 <= len(batch["tasks"]) <= 4 or not 1 <= len(batch["outcomes"]) <= 6:
        raise ValueError("A batch needs 1–4 tasks and 1–6 observable outcomes")
    if not nonempty([batch["title"], batch["objective"], *batch["outcomes"]]):
        raise ValueError("Batch objective and outcomes cannot be empty")
    task_ids = [t["id"] for t in batch["tasks"]]
    if len(task_ids) != len(set(task_ids)):
        raise ValueError("Task IDs must be unique within a batch")
    for task in batch["tasks"]:
        if not nonempty([task["id"], task["title"], task["brief"]]) or not nonempty(task["done_when"]):
            raise ValueError("Tasks need a brief and observable acceptance criteria")
        if any(not isinstance(c, str) or not c.strip() for c in task["checks"]):
            raise ValueError("Checks must be catalogue names or nonempty shell commands")


def batch_checklist(batch):
    return ([{"id": f"{batch['id']}:outcome-{n}", "workflow": value,
              "status": "pending", "evidence": "Not yet verified"}
             for n, value in enumerate(batch["outcomes"], 1)] +
            [{"id": f"{batch['id']}:task-{task['id']}", "workflow": task["title"],
              "status": "pending", "evidence": "Not yet verified"}
             for task in batch["tasks"]])


def prepare(runner):
    """Called with the coordinator lock held. Never overwrite an active process."""
    if not runner.outer_settings().get("enabled") or "outer" in runner.state:
        return
    s = runner.state
    previous = copy.deepcopy(s.get("plan"))
    s["outer"] = {"version": 1, "history": [], "hopper": [], "last_decision": None,
                  "current_batch": None, "roadmap": copy.deepcopy((previous or {}).get("checklist", [])),
                  "enabled_at": time.time()}
    if previous and s["status"] != "complete":
        # Preserve the running/queued assignment and all old roadmap evidence.
        # The inner checklist becomes the bounded acceptance contract, while the
        # old whole-project checklist remains visible as the long-term roadmap.
        batch = {"id": "adopted-current-work", "title": "Finish the current accepted assignment",
                 "objective": previous["worker_prompt"],
                 "outcomes": list(previous["acceptance_criteria"]), "out_of_scope": [],
                 "tasks": [{"id": "current", "title": "Complete and review the current assignment",
                            "brief": previous["worker_prompt"], "done_when": previous["acceptance_criteria"],
                            "checks": previous["checks"]}]}
        s["outer"]["current_batch"] = batch
        s["outer"]["adopted_plan"] = previous
        s["outer"]["batch_start_round"] = s["rounds"]
        s["plan"]["checklist"] = batch_checklist(batch)
    else:
        if previous:
            s["outer"]["history"].append({"id": "legacy-mission", "title": "Original viewer roadmap",
                "completed_at": time.time(), "review": previous, "report": s.get("report"),
                "receipts": s.get("receipts", []), "legacy": True})
        s["phase"] = "director"
        if s["status"] == "complete":
            s.update(status="ready", message="Previous work completed; Director will select the next batch.")
    runner.save()


def director_prompt(runner):
    s, outer = runner.state, runner.state["outer"]
    prompt = ("Review the project after the last accepted batch and choose what is most worth doing next. "
              "Inspect current source and the saved inventory, not only reports. Compare 3–6 concrete candidates "
              "across cohesion, feature gaps, shared-library improvements and technical debt. "
              "Choose ONE cohesive batch with 1–4 ordered tasks, or explain why stopping is wiser. "
              "Do not dispatch a task already completed, and do not invent success or GUI evidence.\n")
    prompt += ("\nIndependent checks may be names from this catalogue or any shell command "
               "(run from the workspace root):\n" + json.dumps(runner.config["checks"]))
    timings = {n: h["seconds"] for n, h in s.get("check_history", {}).items() if h.get("seconds") is not None}
    if timings:
        prompt += "\nMeasured duration of each check's latest run, in seconds (recurring cost per worker turn):\n" + json.dumps(timings)
    prompt += "\nLong-term roadmap (historical evidence, inspect freshness):\n" + json.dumps(outer["roadmap"])
    prompt += "\nPrevious hopper (reconsider deferred items; they are not automatic promises):\n" + json.dumps(outer["hopper"])
    prompt += "\nCompleted batch IDs:\n" + json.dumps([b["id"] for b in outer["history"]])
    prompt += "\nRecent completed batches and evidence:\n" + json.dumps(outer["history"][-5:])
    prompt += "\nCurrent working changes:\n" + json.dumps(runner.evidence())
    return prompt


def dispatch(runner, decision):
    outer = runner.state["outer"]
    outer["last_decision"] = copy.deepcopy(decision)
    outer["hopper"] = copy.deepcopy(decision["candidates"])
    if decision["action"] == "stop":
        runner.state.update(status="paused", message="Director stopped: " + decision["rationale"])
        return False
    batch = copy.deepcopy(decision["batch"])
    outer["current_batch"] = batch
    outer["batch_start_round"] = runner.state["rounds"]
    outer["contract"] = batch_checklist(batch)
    runner.state.update(phase="orchestrator", plan=None, report=None, receipts=[])
    runner.state.pop("operator_replan", None)
    return True


def finish_batch(runner, plan):
    outer = runner.state["outer"]
    batch = outer["current_batch"]
    if not batch:
        raise ValueError("No active batch to finish")
    required = {item["id"] for item in batch_checklist(batch)}
    verified = {item["id"] for item in plan["checklist"] if item["status"] == "verified"}
    if not required.issubset(verified):
        raise ValueError("Cannot finish until all batch tasks and outcomes are verified")
    if any(b["id"] == batch["id"] for b in outer["history"]):
        raise ValueError("Batch already completed")
    outer["history"].append({**copy.deepcopy(batch), "completed_at": time.time(),
                             "review": copy.deepcopy(plan), "report": copy.deepcopy(runner.state["report"]),
                             "receipts": copy.deepcopy(runner.state["receipts"])})
    for item in outer["hopper"]:
        if item["id"] == batch["id"]:
            item["disposition"] = "completed"
    outer["current_batch"] = None
    runner.state["phase"] = "director"
    runner.state["message"] = "Batch accepted. Director will review priorities and refill the hopper."


def contract_prompt(runner):
    batch = runner.state["outer"]["current_batch"]
    if not batch:
        return ""
    return ("\nOUTER-LOOP BATCH CONTRACT: the mission is the long-term direction, not the scope "
            "of this batch. Execute only this batch. Keep every task/outcome checklist ID below. "
            "action=complete means this batch is accepted and ALL its tasks/outcomes are verified; "
            "the Director will then select the next batch. Do not expand to the whole roadmap. "
            "The historical project checklist is preserved separately. Do not repeat the initial inventory.\n" +
            json.dumps({"batch": batch, "required_checklist": batch_checklist(batch)}))


def guard_contract(runner, plan):
    batch = runner.state.get("outer", {}).get("current_batch")
    if not batch:
        return
    required = {item["id"] for item in batch_checklist(batch)}
    if not required.issubset({item["id"] for item in plan["checklist"]}):
        raise ValueError("Orchestrator omitted required batch tasks or outcomes")
    # Task checks are suggestions for the worker, not a required suite: the worker
    # verifies its own work and the orchestrator judges that evidence.
