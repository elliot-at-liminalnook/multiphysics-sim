"""Written continuation regressions, deliberately unexecuted in T43.

The fixture topology matches cad/tests/test_motion.py::crank_slider. These
checks compare successive headless receipts with one persistent reference
PoseModel, including dead-centre crossings, and never run simulation/hardware.
"""
import math
import unittest
from copy import deepcopy
import numpy as np
from .document import Document, Node
from .robotics import Joint
from .pose import PoseModel
from .motion_service import sample, sample_model, identity
from .motion_continuation import restore_prior
from .kernel import KernelError
from .experiments import RevisionConflict


def crank_slider():
    doc = Document()
    for nid in ('base', 'crank', 'rod', 'foot', 'motor'):
        doc.nodes[nid] = Node(nid, 'body', nid)
    joints = {
        'input': Joint('revolute', 'base', 'crank', (0, 0, 0), motor='motor'),
        'link': Joint('revolute', 'crank', 'rod', (75, 0, 0)),
        # Unset passive slider limits must remain unlimited, even though its
        # display widget's fallback range is only +/-100 mm.
        'slide': Joint('prismatic', 'base', 'foot', (175, 0, 0), axis=(1, 0, 0)),
        'closure': Joint('loop_revolute', 'rod', 'foot', (175, 0, 0)),
    }
    for nid, joint in joints.items():
        doc.nodes[nid] = Node(nid, 'joint', nid, joint=joint)
    return doc


def receipt(answer):
    return {'identity': deepcopy(answer['identity']), 'positions': dict(answer['positions'])}


def request(doc, degrees, prior=None):
    return {'document_id': doc.document_id, 'expected_revision': doc.revision,
            'positions': {'input': math.radians(degrees)}, 'time': 0., 'prior': prior}


class ContinuationContract(unittest.TestCase):
    def assert_reference(self, got, reference):
        self.assertEqual(set(got['positions']), set(reference['positions']))
        for jid, value in reference['positions'].items():
            self.assertAlmostEqual(got['positions'][jid], float(value), places=7)
        for nid, matrix in reference['matrices'].items():
            np.testing.assert_allclose(got['matrices'][nid], matrix, atol=1e-7, rtol=0.)
        self.assertAlmostEqual(got['closure_error_mm'], reference['closure_error_mm'], places=7)

    def test_explicit_alternate_assembly_survives_dead_centre_without_home_reset(self):
        doc = crank_slider()
        doc.nodes['input'].joint.lower = -2 * math.pi
        doc.nodes['input'].joint.upper = 2 * math.pi
        # The alternate assembly's continuous passive angle passes -2pi.
        doc.nodes['link'].joint.lower = -4 * math.pi
        doc.nodes['link'].joint.upper = 4 * math.pi
        theta = math.radians(45)
        root = math.sqrt(100**2 - (75 * math.sin(theta))**2)
        prior = {'identity': identity(doc), 'positions': {
            'input': theta,
            'link': math.atan2(-75 * math.sin(theta), -root) - theta,
            'slide': 75 * math.cos(theta) - root - 175,
        }}
        persistent = PoseModel(doc)
        restore_prior(persistent, prior, identity(doc))
        for degrees in (45, 70, 110, 170, 190, 225):
            theta = math.radians(degrees)
            got = sample(doc, request(doc, degrees, prior))
            self.assert_reference(got, sample_model(persistent, {'input': theta}))
            self.assertTrue(got['prior_applied'])
            alternate = 75 * math.cos(theta) - math.sqrt(100**2 - (75 * math.sin(theta))**2) - 175
            self.assertAlmostEqual(got['positions']['slide'], alternate, places=5)
            if degrees == 45:
                unseeded = sample(doc, request(doc, degrees))
                self.assertFalse(unseeded['prior_applied'])
                self.assertGreater(abs(unseeded['positions']['slide'] - got['positions']['slide']), 100.)
            prior = receipt(got)

    def test_successive_headless_samples_follow_persistent_branch_across_dead_centre(self):
        doc = crank_slider(); persistent = PoseModel(doc); prior = None
        for degrees in (-170, -30, 0, 30, 170, 0, -130, 0):
            target = {'input': math.radians(degrees)}
            expected = sample_model(persistent, target)
            got = sample(doc, request(doc, degrees, prior))
            self.assert_reference(got, expected)
            analytic = 75 * math.cos(target['input']) + math.sqrt(100**2 - (75 * math.sin(target['input']))**2) - 175
            self.assertAlmostEqual(got['positions']['slide'], analytic, places=5)
            prior = receipt(got)

    def test_arbitrary_program_seek_continues_current_reference_branch(self):
        doc = crank_slider(); persistent = PoseModel(doc)
        initial = sample(doc, request(doc, -170))
        sample_model(persistent, {'input': math.radians(-170)})
        program = {'name': 'seek', 'duration': 1., 'loop': False,
                   'tracks': [{'joint': 'input', 'unit': 'deg', 'keys': [[0., -170.], [1., 130.]]}]}
        body = request(doc, 0, receipt(initial)); body.update(program=program, time=1.)
        got = sample(doc, body)
        self.assert_reference(got, sample_model(persistent, {'input': math.radians(130)}))

    def test_bad_prior_is_refused_before_seeding_and_kept_unmodified(self):
        doc = crank_slider(); prior = receipt(sample(doc, request(doc, -45)))
        for mutate, error in (
            (lambda p: p['identity'].update(revision=doc.revision + 1), RevisionConflict),
            (lambda p: p['identity'].update(document_id='replacement'), RevisionConflict),
            (lambda p: p['identity'].update(source_kind='experiment'), RevisionConflict),
            (lambda p: p['positions'].pop('slide'), KernelError),
            (lambda p: p['positions'].update(extra=0.), KernelError),
            (lambda p: p['positions'].update(slide=float('nan')), KernelError),
            (lambda p: p['positions'].update(link=True), KernelError),
            (lambda p: p['positions'].update(input=4.), KernelError),
            (lambda p: p['positions'].update(slide=p['positions']['slide'] + 1.), KernelError),
        ):
            changed = deepcopy(prior); mutate(changed)
            model = PoseModel(doc); before = dict(model.last_positions)
            with self.assertRaises(error): restore_prior(model, changed, identity(doc))
            self.assertEqual(model.last_positions, before)
            self.assertEqual(prior['identity'], identity(doc))

    def test_transmission_prior_cannot_invent_an_inconsistent_driven_coordinate(self):
        doc = Document()
        for index in range(2):
            part = f'part{index}'; jid = f'joint{index}'
            doc.nodes[part] = Node(part, 'body', part)
            doc.nodes[jid] = Node(jid, 'joint', jid,
                                  joint=Joint('revolute', None, part, (0, 0, 0), home=.2))
        doc.robot_settings['transmissions'] = [{'driver_joint': 'joint0', 'driven_joint': 'joint1', 'ratio': 5}]
        initial = sample(doc, {'document_id': doc.document_id, 'expected_revision': doc.revision,
                              'positions': {'joint0': 1.2}, 'time': 0.})
        prior = receipt(initial); prior['positions']['joint1'] += .01
        with self.assertRaises(KernelError):
            restore_prior(PoseModel(doc), prior, identity(doc))

    def test_preview_export_and_interleaved_consumers_have_separate_receipts(self):
        doc = crank_slider()
        a = receipt(sample(doc, request(doc, -45)))
        b = receipt(sample(doc, request(doc, 45)))
        expected_a = sample(doc, request(doc, -130, deepcopy(a)))
        expected_b = sample(doc, request(doc, 130, deepcopy(b)))
        # Interleave unrelated requests; there is no service-owned global seed.
        sample(doc, request(doc, 0))
        got_a = sample(doc, request(doc, -130, a))
        got_b = sample(doc, request(doc, 130, b))
        self.assertEqual(got_a, expected_a); self.assertEqual(got_b, expected_b)
        self.assertAlmostEqual(a['positions']['input'], math.radians(-45))
        self.assertAlmostEqual(b['positions']['input'], math.radians(45))
        # A cancellation/stale caller can drop a computed answer without ever
        # advancing its last published receipt; subsequent reuse is deterministic.
        sample(doc, request(doc, -170, a))
        self.assertEqual(sample(doc, request(doc, -130, a)), expected_a)
