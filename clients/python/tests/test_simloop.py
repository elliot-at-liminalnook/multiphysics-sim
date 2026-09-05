"""Scripted conversations through in-memory streams, checking the frames Loop writes."""

import io
import json
import os
import socket
import tempfile
import threading
import time
import unittest

from simloop import Channel, Frame, Loop, ProtocolError

HELLO = '{"type":"hello","element":"controller","period":0.001,"sensors":[{"name":"angle","unit":"rad"},{"name":"speed","unit":"rad/s"}],"actuators":[{"name":"voltage","unit":"V"},{"name":"brake","unit":"1"}]}'


def make_loop(*lines, text=False):
    script = "".join(line + "\n" for line in lines)
    reader, writer = (io.StringIO(script), io.StringIO()) if text else (io.BytesIO(script.encode()), io.BytesIO())
    loop = Loop(reader, writer)
    written = lambda: [json.loads(l) for l in (writer.getvalue() if text else writer.getvalue().decode()).splitlines()]
    return loop, writer, written


class HelloTest(unittest.TestCase):
    def test_contract_and_ready(self):
        loop, writer, written = make_loop(HELLO)
        c = loop.contract
        self.assertEqual((c.element, c.period), ("controller", 0.001))
        self.assertEqual(c.sensors, [Channel("angle", "rad"), Channel("speed", "rad/s")])
        self.assertEqual(c.actuators, [Channel("voltage", "V"), Channel("brake", "1")])
        self.assertEqual(writer.getvalue(), b'{"type":"ready"}\n')

    def test_rejects_non_hello(self):
        with self.assertRaises(ProtocolError):
            make_loop('{"type":"sample","seq":0,"t":0,"sensors":[]}')
        with self.assertRaises(ProtocolError):
            make_loop()
        with self.assertRaises(ProtocolError):
            make_loop('{"type":"hello","element":"c","period":0.1,"sensors":[{"name":"x"}],"actuators":[]}')


class FrameTest(unittest.TestCase):
    def test_frames_and_replies(self):
        loop, _, written = make_loop(HELLO, '{"type":"sample","seq":0,"t":0.0,"sensors":[0.1,2]}', '{"type":"sample","seq":1,"t":0.001,"sensors":[0.09,1.5]}', '{"type":"close"}')
        frames = []
        for frame in loop:
            frames.append(frame)
            if frame.seq == 0:
                loop.send(voltage=2.5)
            else:
                loop.send([2.4, 1.0])
        self.assertEqual(len(frames), 2)
        self.assertIsInstance(frames[0], Frame)
        self.assertEqual((frames[0].seq, frames[0].t, frames[0]["angle"], frames[0].values), (0, 0.0, 0.1, [0.1, 2.0]))
        self.assertEqual(frames[1].sensors, {"angle": 0.09, "speed": 1.5})
        self.assertEqual(written(), [
            {"type": "ready"},
            {"type": "act", "seq": 0, "actuators": [2.5, 0.0]},
            {"type": "act", "seq": 1, "actuators": [2.4, 1.0]},
        ])

    def test_missing_actuators_hold_previous_value(self):
        loop, _, written = make_loop(HELLO, '{"type":"sample","seq":0,"t":0,"sensors":[0,0]}', '{"type":"sample","seq":1,"t":0,"sensors":[0,0]}', '{"type":"sample","seq":2,"t":0,"sensors":[0,0]}')
        it = iter(loop)
        next(it); loop.send(brake=0.5)
        next(it); loop.send([1.0])
        next(it)  # unanswered; the next read (EOF) sends the held values first
        self.assertRaises(StopIteration, next, it)
        self.assertEqual([f["actuators"] for f in written()[1:]], [[0.0, 0.5], [1.0, 0.5], [1.0, 0.5]])

    def test_text_streams(self):
        loop, _, written = make_loop(HELLO, '{"type":"sample","seq":0,"t":0.5,"sensors":[1,1]}', text=True)
        for frame in loop:
            loop.send(voltage=-frame.t)
        self.assertEqual(written()[1], {"type": "act", "seq": 0, "actuators": [-0.5, 0.0]})

    def test_eof_ends_iteration(self):
        loop, _, _ = make_loop(HELLO)
        self.assertEqual(list(loop), [])


class ErrorTest(unittest.TestCase):
    def sample(self, *lines):
        loop, _, _ = make_loop(HELLO, *lines)
        return lambda: next(iter(loop))

    def test_seq_mismatch(self):
        self.assertRaisesRegex(ProtocolError, "expected seq 0", self.sample('{"type":"sample","seq":1,"t":0,"sensors":[0,0]}'))

    def test_unknown_type(self):
        self.assertRaisesRegex(ProtocolError, "unexpected frame", self.sample('{"type":"pause"}'))

    def test_malformed(self):
        self.assertRaisesRegex(ProtocolError, "malformed", self.sample('{"type":"sample",'))
        self.assertRaisesRegex(ProtocolError, "malformed", self.sample('[1,2]'))
        self.assertRaisesRegex(ProtocolError, "malformed", self.sample('{"type":"sample","seq":0,"t":"soon","sensors":[0,0]}'))
        self.assertRaisesRegex(ProtocolError, "expected 2 sensors", self.sample('{"type":"sample","seq":0,"t":0,"sensors":[0]}'))

    def test_bad_send(self):
        loop, _, _ = make_loop(HELLO, '{"type":"sample","seq":0,"t":0,"sensors":[0,0]}')
        self.assertRaises(ProtocolError, loop.send, [1.0])  # nothing to answer yet
        next(iter(loop))
        self.assertRaises(ProtocolError, loop.send, [1.0, 2.0, 3.0])
        self.assertRaises(ProtocolError, loop.send, throttle=1.0)
        self.assertRaises(ProtocolError, loop.send, voltage=float("nan"))


class UnixSocketTest(unittest.TestCase):
    def test_listen_unix(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "sim.sock")
            result = {}

            def controller():
                with Loop.listen_unix(path) as loop:
                    result["contract"] = loop.contract
                    for frame in loop:
                        loop.send(voltage=-2.0 * frame["angle"])

            thread = threading.Thread(target=controller)
            thread.start()
            sim = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            for _ in range(100):
                try:
                    sim.connect(path)
                    break
                except OSError:
                    time.sleep(0.02)
            with sim, sim.makefile("rb") as rd, sim.makefile("wb") as wr:
                def say(line):
                    wr.write((line + "\n").encode()); wr.flush()
                    return json.loads(rd.readline())
                self.assertEqual(say(HELLO), {"type": "ready"})
                self.assertEqual(say('{"type":"sample","seq":0,"t":0,"sensors":[0.25,0]}'), {"type": "act", "seq": 0, "actuators": [-0.5, 0.0]})
                wr.write(b'{"type":"close"}\n'); wr.flush()
            thread.join(5)
            self.assertFalse(thread.is_alive())
            self.assertEqual(result["contract"].element, "controller")


if __name__ == "__main__":
    unittest.main()
