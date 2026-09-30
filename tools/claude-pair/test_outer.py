import copy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import pair
import outer_loop as outer
import test_pair as fixtures


def decision(n=1):
    bid = f'batch-{n}'
    return {'decisions': [], 'coordination_notes': [], 'action': 'select', 'summary': 'Make one workflow coherent',
            'rationale': 'This closes an observed workflow gap with a small reusable change.',
            'selected_id': bid, 'candidates': [
                {'id': bid if i == 0 else f'deferred-{i}', 'title': f'Candidate {i}',
                 'category': ['cohesion', 'library', 'tech_debt'][i],
                 'problem': 'Concrete observed gap', 'evidence': ['tracked.txt:1'],
                 'benefit': 'One fewer manual step', 'leverage': 'Existing shared code',
                 'effort': 'small', 'risk': 'Needs verification',
                 'disposition': 'select' if i == 0 else 'defer', 'reason': 'Best next step' if i == 0 else 'Lower value now'}
                for i in range(3)],
            'batch': {'id': bid, 'title': 'A cohesive workflow', 'objective': 'Close the observed gap',
                      'outcomes': ['Proof is readable'], 'out_of_scope': ['Unrelated rewrites'],
                      'tasks': [{'id': 'proof', 'title': 'Create proof', 'brief': 'Write proof.txt',
                                 'done_when': ['Proof exists'], 'checks': ['diff']}]}}


class OuterTests(unittest.TestCase):
    def setup_run(self, tmp):
        root = fixtures.PairTests().initialize_fixture(Path(tmp).resolve())
        config = pair.read_json(root / 'config.json')
        config['max_rounds'] = 10
        pair.write_json(root / 'config.json', config)
        pair.configure_outer(root, True, 2)
        return root

    def fake_call(self, runner, role, prompt, scope=None):
        runner.state['calls'] += 1
        runner.state['cost_usd'] += .1
        if role == 'director':
            return decision(len(runner.state['outer']['history']) + 1)
        if role == 'worker':
            (runner.repo / 'proof.txt').write_text('proof\n')
            return copy.deepcopy(fixtures.REPORT)
        p = fixtures.plan()
        p['checklist'] = outer.batch_checklist(runner.state['outer']['current_batch'])
        if runner.state.get('report'):
            p.update(action='complete', review='accept')
            for item in p['checklist']:
                item.update(status='verified', evidence='Read proof.txt; independent diff passed')
        return p

    def test_two_batches_review_then_refill_and_cumulative_limits(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.setup_run(tmp)
            roles = []
            def call(runner, role, prompt, scope=None):
                roles.append(role)
                return self.fake_call(runner, role, prompt)
            with patch.object(pair.Runner, 'call', call):
                pair.Runner(root).run()
            s = pair.read_json(root / 'state.json')
            self.assertEqual(roles, ['director', 'orchestrator', 'worker', 'orchestrator'] * 2)
            self.assertEqual(len(s['outer']['history']), 2)
            self.assertEqual(s['rounds'], 2)
            self.assertAlmostEqual(s['cost_usd'], .8)
            self.assertGreater(s['elapsed_seconds'], 0)
            self.assertEqual(s['phase'], 'director')
            self.assertIn('batch ceiling', s['message'].replace('Completed-batch', 'batch'))
            self.assertTrue(all(b['receipts'][0]['exit_code'] == 0 for b in s['outer']['history']))

    def test_failed_verification_never_refills(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.setup_run(tmp)
            def verify(runner):
                runner.state['receipts'] = [{'name': 'diff', 'exit_code': 1}]
            with patch.object(pair.Runner, 'call', lambda runner, role, prompt, scope=None: self.fake_call(runner, role, prompt)), patch.object(pair.Runner, 'verify', verify):
                pair.Runner(root).run()
            s = pair.read_json(root / 'state.json')
            self.assertEqual(s['status'], 'blocked')
            self.assertEqual(s['outer']['history'], [])
            self.assertEqual(s['outer']['current_batch']['id'], 'batch-1')

    def test_migration_keeps_assignment_roadmap_and_report(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fixtures.PairTests().initialize_fixture(Path(tmp).resolve())
            runner = pair.Runner(root)
            runner.state.update(plan=fixtures.plan(), report=copy.deepcopy(fixtures.REPORT), phase='verify')
            runner.save()
            pair.configure_outer(root, True, 8)
            s = pair.read_json(root / 'state.json')
            self.assertEqual(s['outer']['roadmap'], fixtures.plan()['checklist'])
            self.assertEqual(s['plan']['worker_prompt'], fixtures.plan()['worker_prompt'])
            self.assertEqual(s['report'], fixtures.REPORT)
            self.assertEqual(s['phase'], 'verify')
            self.assertTrue(s['plan']['checklist'][0]['id'].startswith('adopted-current-work:'))

    def test_active_upgrade_does_not_write_state_or_change_budgets(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fixtures.PairTests().initialize_fixture(Path(tmp).resolve())
            before = (root / 'state.json').read_bytes()
            config = (root / 'config.json').read_bytes()
            with pair.lock(root), patch.object(pair.subprocess, 'Popen') as launch:
                pair.configure_outer(root, True, 8)
                self.assertEqual(launch.call_count, 1)
            self.assertEqual((root / 'state.json').read_bytes(), before)
            self.assertEqual((root / 'config.json').read_bytes(), config)
            self.assertTrue(pair.read_json(root / 'outer-settings.json')['enabled'])

    def test_decision_contract_and_duplicate_guards(self):
        s = {'outer': {'history': []}}
        d = decision()
        pair.validate(d, outer.DIRECTOR_SCHEMA)
        outer.guard_decision(d, s, {'diff': []})
        s['outer']['history'] = [{'id': 'batch-1'}]
        with self.assertRaisesRegex(ValueError, 'repeat'):
            outer.guard_decision(d, s, {'diff': []})
        s['outer']['history'] = []
        d['candidates'][1]['evidence'] = []
        with self.assertRaisesRegex(ValueError, 'evidence'):
            outer.guard_decision(d, s, {'diff': []})
        with tempfile.TemporaryDirectory() as tmp:
            root = self.setup_run(tmp)
            runner = pair.Runner(root)
            outer.dispatch(runner, decision())
            with self.assertRaisesRegex(ValueError, 'omitted'):
                outer.guard_contract(runner, fixtures.plan())

    def test_director_can_stop_and_disabled_loop_does_not_dispatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.setup_run(tmp)
            d = decision()
            d.update(action='stop', selected_id='', candidates=[])
            d['batch']['tasks'] = []
            outer.guard_decision(d, pair.Runner(root).state, {'diff': []})
            with patch.object(pair.Runner, 'call', return_value=d) as call:
                pair.Runner(root).run()
                self.assertEqual(call.call_count, 1)
            self.assertIn('Director stopped', pair.read_json(root / 'state.json')['message'])
            pair.configure_outer(root, False, 2)
            with patch.object(pair.Runner, 'call') as call:
                pair.Runner(root).run()
                call.assert_not_called()

    def test_watcher_preserves_stop_and_does_not_restart_paused_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fixtures.PairTests().initialize_fixture(Path(tmp).resolve())
            pair.write_json(root / 'outer-settings.json', {'enabled': True, 'max_batches': 8})
            runner = pair.Runner(root)
            runner.state.update(status='paused', plan=fixtures.plan())
            runner.save()
            (root / 'STOP').touch()
            with patch.object(pair.Runner, 'run') as run:
                pair.watch_outer(root)
                run.assert_not_called()
            self.assertTrue((root / 'STOP').exists())
            self.assertIn('outer', pair.read_json(root / 'state.json'))


if __name__ == '__main__':
    unittest.main()
