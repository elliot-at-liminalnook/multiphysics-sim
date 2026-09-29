import unittest
import workflow

class WorkflowTests(unittest.TestCase):
    def test_assignment_and_review_are_distinct(self):
        self.assertEqual(workflow.call_stage('orchestrator','Read the mission'), 'assign')
        self.assertEqual(workflow.call_stage('orchestrator','Review the worker using this evidence: {}'), 'review')
        s={'phase':'orchestrator','status':'running','report':{'status':'done'}}
        w=workflow.describe(s,True,[],[],100)
        self.assertEqual(w['current'],'review')
        self.assertTrue(next(n for n in w['nodes'] if n['id']=='review')['running'])
        self.assertEqual(sum(n['running'] for n in w['nodes']),1)

    def test_paused_worker_is_not_live(self):
        s={'phase':'worker','status':'paused','message':'Disk space low'}
        w=workflow.describe(s,False,[],[],100)
        self.assertFalse(any(n['running'] for n in w['nodes']))
        self.assertEqual(w['nodes'][2]['status'],'Paused here')
        self.assertEqual(w['message'],'Disk space low')

    def test_history_does_not_imply_current_success(self):
        records=[{'stage':'assign','finished':True,'id':'old-assign'},
                 {'stage':'review','finished':True,'id':'old-review'}]
        s={'phase':'verify','status':'running'}
        w=workflow.describe(s,True,records,[],100)
        self.assertEqual(w['nodes'][1]['call']['id'],'old-assign')
        self.assertEqual(w['nodes'][4]['call']['id'],'old-review')
        self.assertEqual(w['nodes'][3]['status'],'Running')
        self.assertEqual(w['nodes'][3]['checks'],[])

    def test_failed_checks_and_completed_batches(self):
        s={'phase':'orchestrator','status':'blocked','report':{},'outer':{'history':[{'id':'old','legacy':True},{'id':'new'}]}}
        w=workflow.describe(s,False,[],[{'exit_code':1}],100)
        self.assertEqual(w['nodes'][3]['status'],'Last checks failed')
        self.assertEqual(w['completed_batches'],1)

if __name__=='__main__':
    unittest.main()
