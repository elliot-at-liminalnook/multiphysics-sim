"""Read-only presentation of coordinator phases; never changes run state."""
from pathlib import Path

STEPS = [
    ('director', 'Choose a batch', 'Director', 'Compare opportunities and select worthwhile work.'),
    ('assign', 'Write the assignment', 'Orchestrator', 'Turn the selected batch into one specific worker prompt.'),
    ('worker', 'Implement', 'Worker', 'Edit the code and run focused development checks.'),
    ('verify', 'Run checks', 'Coordinator', 'Run the agreed independent checks against the changed code.'),
    ('review', 'Review the result', 'Orchestrator', 'Inspect evidence, request fixes, or accept the completed batch.'),
]


def call_stage(role, prompt):
    if role == 'orchestrator':
        return 'review' if 'Review the worker using this evidence:' in prompt else 'assign'
    return role


def describe(state, active, records, checks, now):
    phase = state['phase']
    current = ('review' if state.get('report') else 'assign') if phase == 'orchestrator' else phase
    latest = {step: next((r for r in reversed(records) if r['stage'] == step), None) for step, *_ in STEPS}
    nodes = []
    for key, title, owner, purpose in STEPS:
        call = latest[key]
        here = key == current
        running = active and here
        status = ('Running' if running else 'Paused here' if here and state['status'] == 'paused'
                  else 'Needs attention' if here and state['status'] == 'blocked'
                  else 'Up next' if here and state['status'] != 'complete'
                  else 'Last turn finished' if call and call['finished']
                  else 'Interrupted' if call else 'Waiting')
        if key == 'verify' and not here:
            status = 'Last checks passed' if checks and all(c['exit_code'] == 0 for c in checks) else 'Last checks failed' if checks else 'Waiting'
        nodes.append({'id': key, 'title': title, 'owner': owner, 'purpose': purpose,
                      'current': here, 'running': running, 'status': status,
                      'call': call, 'checks': checks if key == 'verify' else []})
    return {'current': current, 'nodes': nodes,
            'completed_batches': sum(not b.get('legacy') for b in state.get('outer', {}).get('history', [])),
            'message': state.get('message', ''), 'active': active}
