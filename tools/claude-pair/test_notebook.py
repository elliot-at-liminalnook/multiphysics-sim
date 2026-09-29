import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from concurrent.futures import ThreadPoolExecutor

import pair
import shared_notebook as book
import test_pair as fixtures

class NotebookTests(unittest.TestCase):
    def test_append_is_idempotent_ordered_and_concurrent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            def add(n):
                book.append(root,{'id':str(n),'author':'worker','kind':'Note','summary':str(n),'notes':['shared'], 'source':'test'})
            with ThreadPoolExecutor(max_workers=4) as pool:
                list(pool.map(add,range(12)))
            before=(root/'shared/journal.jsonl').read_bytes()
            add(1)
            self.assertEqual((root/'shared/journal.jsonl').read_bytes(),before)
            entries=book.entries(root)
            self.assertEqual([e['sequence'] for e in entries],list(range(1,13)))
            self.assertEqual(len({e['id'] for e in entries}),12)
            self.assertEqual((root/'shared/JOURNAL.md').read_text(),book.render(entries))

    def test_partial_journal_is_preserved_and_blocks_append(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);(root/'shared').mkdir()
            path=root/'shared/journal.jsonl';path.write_text('{"incomplete":')
            with self.assertRaisesRegex(ValueError,'incomplete'):
                book.append(root,{'id':'next'})
            self.assertEqual(path.read_text(),'{"incomplete":')

    def test_notes_reach_next_role_with_shared_read_access(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=fixtures.PairTests().initialize_fixture(Path(tmp).resolve())
            runner=pair.Runner(root)
            calls=[]
            def process(argv,prefix,stdin=None):
                calls.append((argv,stdin))
                role='orchestrator' if len(calls)==1 else 'worker'
                data=fixtures.plan() if role=='orchestrator' else copy.deepcopy(fixtures.REPORT)
                data['coordination_notes']=['Worker: retain the measurement artifact; see src/example.rs.'] if role=='orchestrator' else ['Orchestrator: question answered; artifact retained.']
                sid=argv[argv.index('--session-id')+1]
                out=prefix.with_suffix('.stdout')
                pair.write_json(out,{'session_id':sid,'total_cost_usd':.1,'subtype':'success','structured_output':data})
                return 0,out,prefix.with_suffix('.stderr')
            with patch.object(runner,'process',process):
                runner.call('orchestrator','Make an assignment')
                runner.call('worker','Follow the assignment')
            self.assertIn('retain the measurement artifact',calls[1][1])
            for argv, prompt in calls:
                self.assertIn('DISK-SPACE PREFLIGHT (fresh for this turn)', prompt)
                self.assertIn('free_gib', prompt)
                self.assertIn('build_planning_baseline_gib', prompt)
            self.assertIn(str(root/'shared/SYSTEM.md'),calls[1][1])
            self.assertEqual(calls[1][0][calls[1][0].index('--add-dir')+1],str(root))
            self.assertIn('--dangerously-skip-permissions',calls[0][0])
            self.assertNotIn('--tools',calls[0][0])
            rows=book.entries(root)
            self.assertEqual([e['author'] for e in rows],['coordinator','orchestrator','worker'])
            self.assertIn('artifact retained',rows[-1]['notes'][0])
            self.assertIn('CURRENT.md',book.context(root,'director'))
            self.assertIn('artifact retained',book.context(root,'director'))

    def test_import_completed_history_once_and_skip_interrupted(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=fixtures.PairTests().initialize_fixture(Path(tmp).resolve())
            (root/'logs/0001-worker.prompt.md').write_text('Old assignment')
            pair.write_json(root/'logs/0001-worker.stdout',{'subtype':'success','structured_output':fixtures.REPORT})
            (root/'logs/0002-worker.prompt.md').write_text('Interrupted')
            (root/'logs/0002-worker.stdout').write_text('{"type":"assistant"}\n')
            runner=pair.Runner(root)
            book.setup(root,runner.state,runner.config)
            book.setup(root,runner.state,runner.config)
            rows=book.entries(root)
            self.assertEqual(len(rows),2)
            self.assertTrue(rows[0]['historical'])
            self.assertEqual(rows[0]['id'],'call-0001-worker')
            self.assertIn('Director', (root/'shared/SYSTEM.md').read_text())

if __name__=='__main__':unittest.main()
