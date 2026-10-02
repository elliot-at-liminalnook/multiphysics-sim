"""Written T43 regressions; source-reviewed, deliberately not executed."""
import unittest
from types import SimpleNamespace
from unittest.mock import patch
from . import motion_service, captured_review
from .experiments import RevisionConflict

class HeadlessContract(unittest.TestCase):
    def test_replaced_document_refuses_even_equal_revision(self):
        doc = SimpleNamespace(document_id='replacement', revision=4)
        with self.assertRaises(RevisionConflict):
            motion_service.guard(doc, {'document_id': 'original', 'expected_revision': 4})

    def test_missing_capture_has_no_live_substitution(self):
        record = {'id': 'run', 'document_id': 'captured', 'revision': 2,
                  'provenance': {'physical_hash': 'captured-hash'}}
        value = captured_review.geometry(None, record, 'experiment')
        self.assertEqual(value['nodes'], [])
        self.assertEqual(value['identity']['document_id'], 'captured')
        self.assertEqual(value['identity']['physical_hash'], 'captured-hash')
        self.assertIsNotNone(value['missing_reason'])

    def test_headless_sampling_does_not_need_app_or_pose_panel(self):
        import numpy as np
        model = SimpleNamespace(home={'joint': 0.}, drivers={'joint': 0.},
            last_positions={'joint': .2}, last_error_mm=0., matrices=lambda values: {'part': np.eye(4)})
        doc = SimpleNamespace(document_id='doc', revision=2)
        with patch.object(motion_service, 'PoseModel', return_value=model):
            value = motion_service.sample(doc, {'document_id':'doc','expected_revision':2,
                'positions': {'joint': .2}, 'time': .1})
        self.assertEqual(value['matrices']['part'][0], [1.,0.,0.,0.])
        self.assertEqual(value['positions']['joint'], .2)
        self.assertEqual(value['identity']['source_kind'], 'live_kinematic')

    def test_stale_pose_revision_preserves_program(self):
        doc = SimpleNamespace(document_id='doc', revision=3)
        body = {'document_id':'doc','expected_revision':2, 'program':{'name':'retained'}}
        with self.assertRaises(RevisionConflict): motion_service.guard(doc, body)
        self.assertEqual(body['program']['name'], 'retained')


class CompatibilityStamp(unittest.TestCase):
    def test_helper_keeps_explicit_stale_revision_and_document_identity(self):
        from .client import RoboClient
        client = RoboClient()
        with patch.object(client, 'get', return_value={'document_id':'current','revision':9}):
            body = client._stamped({'document_id':'original','expected_revision':3})
            self.assertEqual(body['document_id'], 'original')
            self.assertEqual(body['expected_revision'], 3)
            self.assertEqual(client._stamped({})['expected_revision'], 9)

class AnalyticReferencePose(unittest.TestCase):
    def definition(self):
        import math
        def joint(kind, child, axis, lower, upper):
            return SimpleNamespace(type=kind, child=child, parent=None, axis=axis,
                pivot=[0.,0.,0.], home=0., stroke=None, lower=lower, upper=upper, motor=None)
        def node(nid, j=None):
            return SimpleNamespace(id=nid, name=nid, joint=j, disabled=False, robot=None)
        return SimpleNamespace(document_id='analytic', revision=1, robot_settings={},
            nodes={'slider':node('slider',joint('prismatic','linear',[1.,0.,0.],-50.,50.)),
                   'hinge':node('hinge',joint('revolute','rotary',[0.,0.,1.],-math.pi,math.pi)),
                   'linear':node('linear'), 'rotary':node('rotary')})

    def test_reference_millimetres_and_radians(self):
        import math
        value=motion_service.sample(self.definition(), {'document_id':'analytic','expected_revision':1,
            'positions':{'slider':25.,'hinge':math.pi/2},'time':0.})
        self.assertAlmostEqual(value['matrices']['linear'][0][3],25.)
        self.assertAlmostEqual(value['matrices']['rotary'][0][0],0.)
        self.assertAlmostEqual(value['matrices']['rotary'][1][0],1.)

    def test_declared_travel_stop_refuses_driver_sample(self):
        from .kernel import KernelError
        with self.assertRaises(KernelError):
            motion_service.sample(self.definition(), {'document_id':'analytic','expected_revision':1,
                'positions':{'slider':51.},'time':0.})

    def test_program_degree_conversion_uses_shared_reference_sampler(self):
        import math
        program={'name':'quarter turn','duration':1.,'loop':False,
            'tracks':[{'joint':'hinge','unit':'deg','keys':[[0.,0.],[1.,90.]]}]}
        value=motion_service.sample(self.definition(), {'document_id':'analytic','expected_revision':1,
            'positions':{},'program':program,'time':1.})
        self.assertAlmostEqual(value['positions']['hinge'],math.pi/2)
        self.assertAlmostEqual(value['matrices']['rotary'][1][0],1.)

    def test_captured_replay_cursor_stamps_actual_nearest_sample(self):
        identity=[[1.,0.,0.,0.],[0.,1.,0.,0.],[0.,0.,1.,0.],[0.,0.,0.,1.]]
        moved=[row[:] for row in identity];moved[0][3]=10.
        result={'trace':{'t':[0.,1.],'poses':{'Link':[identity,moved]},
            'signals':{'speed':[0.,2.]}},'signal_units':{'speed':'m/s'},
            'cad_mapping':[{'name':'Link','id':'captured-part','members':['captured-member']}]}
        record={'id':'captured-run','document_id':'source','revision':2,'provenance':{}}
        value=captured_review.sample(result,record,.8)
        self.assertEqual(value['time'],1.)
        self.assertEqual(value['index'],1)
        self.assertEqual(value['values']['speed'],2.)
        self.assertEqual(value['matrices']['captured-member'][0][3],10.)
        self.assertEqual(value['identity']['source_id'],'captured-run')

class CandidateDecisionRace(unittest.TestCase):
    def test_refusal_while_acceptance_stages_prevents_publication(self):
        import threading
        from unittest.mock import Mock
        from .candidates import Candidates
        from .kernel import KernelError
        doc=SimpleNamespace(document_id='doc',revision=7,_lock=threading.RLock())
        stack=SimpleNamespace(push=Mock())
        service=Candidates(doc,SimpleNamespace(stack=stack),'/unused-fixture-path')
        record={'document_id':'doc','base_revision':7,'state':'draft','label':'candidate'}
        refused={**record,'state':'discarded'}
        with patch.object(service,'get',side_effect=[record,refused]), patch.object(service,'document',return_value=object()):
            with self.assertRaises(KernelError): service.accept('candidate',7)
        stack.push.assert_not_called()
        self.assertEqual(doc.revision,7)
