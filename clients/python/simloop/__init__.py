"""Controller-side client for the simulator's external-controller seam.

The simulator drives a controller in lockstep over newline-delimited JSON,
either through the controller's stdin/stdout (the simulator spawns it) or
over a TCP/Unix socket (the controller listens, the simulator connects).
The simulator speaks first::

    -> {"type":"hello","element":"controller","period":0.001,
        "sensors":[{"name":"angle","unit":"rad"}],
        "actuators":[{"name":"voltage","unit":"V"}]}
    <- {"type":"ready"}
    -> {"type":"sample","seq":0,"t":0.0,"sensors":[0.1]}
    <- {"type":"act","seq":0,"actuators":[2.5]}
    ...
    -> {"type":"close"}

``t`` is simulation time; this module never exposes a wall clock.  A typical
controller is::

    loop = Loop.stdio()
    for frame in loop:
        loop.send(voltage=-2.0 * frame["angle"])

Over stdio nothing but protocol frames may be written to stdout; log to
stderr.
"""

from __future__ import annotations

import io
import json
import math
import os
import socket
import sys
from typing import IO, Any, Dict, Iterator, List, NamedTuple, Optional, Sequence, Tuple, Union

__all__ = ["Channel", "Contract", "Frame", "Loop", "ProtocolError"]

Address = Tuple[str, int]


class ProtocolError(Exception):
    """A frame that violates the protocol: bad JSON, wrong type, wrong seq."""


class Channel(NamedTuple):
    """One sensor or actuator as declared in the simulator's hello."""

    name: str
    unit: str


class Contract:
    """What the simulator declared in its hello frame."""

    def __init__(self, element: str, period: float, sensors: Sequence[Channel], actuators: Sequence[Channel]):
        self.element = element
        self.period = period
        self.sensors: List[Channel] = list(sensors)
        self.actuators: List[Channel] = list(actuators)

    def __repr__(self) -> str:
        return f"Contract(element={self.element!r}, period={self.period!r}, sensors={self.sensors!r}, actuators={self.actuators!r})"

    @classmethod
    def _from_hello(cls, hello: Dict[str, Any]) -> "Contract":
        try:
            channels = lambda key: [Channel(str(c["name"]), str(c["unit"])) for c in hello[key]]
            return cls(str(hello["element"]), float(hello["period"]), channels("sensors"), channels("actuators"))
        except (KeyError, TypeError, ValueError) as e:
            raise ProtocolError(f"malformed hello {hello!r}: {e}") from None


class Frame:
    """One sample from the simulator: sensor values at simulation time ``t``."""

    def __init__(self, seq: int, t: float, names: Sequence[str], values: Sequence[float]):
        self.seq = seq
        self.t = t
        self.values: List[float] = list(values)
        self.sensors: Dict[str, float] = dict(zip(names, self.values))

    def __getitem__(self, name: str) -> float:
        return self.sensors[name]

    def __repr__(self) -> str:
        return f"Frame(seq={self.seq}, t={self.t!r}, sensors={self.sensors!r})"


class Loop:
    """A lockstep session with the simulator.

    Construct with a reader and writer (text or binary file objects), or use
    :meth:`stdio`, :meth:`listen` or :meth:`listen_unix`.  Construction reads
    the hello and replies ``ready``; :attr:`contract` then describes the
    channels.  Iterate to receive frames and :meth:`send` a reply to each.
    Iteration ends cleanly on ``close`` or end of stream.

    If the next frame is requested before the current one has been answered,
    the held actuator values are sent, so a controller that skips a step
    holds its last output rather than deadlocking the simulator.
    """

    def __init__(self, reader: IO[Any], writer: IO[Any]):
        self._reader = reader
        self._writer = writer
        self._owned: List[Any] = []
        self._seq = 0
        self._pending: Optional[Frame] = None
        self._closed = False
        hello = self._read()
        if hello is None:
            raise ProtocolError("stream closed before hello")
        if hello.get("type") != "hello":
            raise ProtocolError(f"expected hello, got {hello!r}")
        self.contract = Contract._from_hello(hello)
        self._sensor_names = [c.name for c in self.contract.sensors]
        self._actuator_index = {c.name: i for i, c in enumerate(self.contract.actuators)}
        self._held: List[float] = [0.0] * len(self.contract.actuators)
        self._write({"type": "ready"})

    # -- constructors --------------------------------------------------------

    @classmethod
    def stdio(cls) -> "Loop":
        """Speak over this process's stdin/stdout (the simulator spawned us)."""
        return cls(sys.stdin.buffer, sys.stdout.buffer)

    @classmethod
    def listen(cls, address: Address) -> "Loop":
        """Listen on a TCP ``(host, port)`` and accept one simulator connection."""
        server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server.bind(address)
        return cls._accept(server)

    @classmethod
    def listen_unix(cls, path: Union[str, "os.PathLike[str]"]) -> "Loop":
        """Listen on a Unix socket at ``path`` and accept one simulator connection."""
        path = os.fspath(path)
        if os.path.exists(path):
            os.unlink(path)
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server.bind(path)
        return cls._accept(server)

    @classmethod
    def _accept(cls, server: socket.socket) -> "Loop":
        with server:
            server.listen(1)
            conn, _ = server.accept()
        if conn.family == socket.AF_INET:
            conn.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        reader, writer = conn.makefile("rb"), conn.makefile("wb")
        loop = cls(reader, writer)
        loop._owned += [reader, writer, conn]
        return loop

    # -- frames --------------------------------------------------------------

    def __iter__(self) -> Iterator[Frame]:
        return self

    def __next__(self) -> Frame:
        if self._closed:
            raise StopIteration
        if self._pending is not None:
            self.send()
        frame = self._read()
        if frame is None or frame.get("type") == "close":
            self._closed = True
            raise StopIteration
        if frame.get("type") != "sample":
            raise ProtocolError(f"unexpected frame {frame!r}")
        try:
            seq, t, values = int(frame["seq"]), float(frame["t"]), [float(v) for v in frame["sensors"]]
        except (KeyError, TypeError, ValueError) as e:
            raise ProtocolError(f"malformed sample {frame!r}: {e}") from None
        if seq != self._seq:
            raise ProtocolError(f"expected seq {self._seq}, got {seq}")
        if len(values) != len(self._sensor_names):
            raise ProtocolError(f"expected {len(self._sensor_names)} sensors, got {len(values)}")
        self._pending = Frame(seq, t, self._sensor_names, values)
        return self._pending

    def send(self, values: Optional[Sequence[float]] = None, **by_name: float) -> None:
        """Answer the current frame.

        ``loop.send([2.5, 0.0])`` sets actuators by position;
        ``loop.send(voltage=2.5)`` by name.  Actuators not mentioned keep
        their previous value (initially 0).
        """
        frame = self._pending
        if frame is None:
            raise ProtocolError("no frame awaiting a reply")
        if values is not None:
            if len(values) > len(self._held):
                raise ProtocolError(f"{len(values)} values for {len(self._held)} actuators")
            self._held[: len(values)] = [float(v) for v in values]
        for name, value in by_name.items():
            if name not in self._actuator_index:
                raise ProtocolError(f"unknown actuator {name!r}; have {list(self._actuator_index)}")
            self._held[self._actuator_index[name]] = float(value)
        if not all(math.isfinite(v) for v in self._held):
            raise ProtocolError(f"non-finite actuator value in {self._held!r}")
        self._write({"type": "act", "seq": frame.seq, "actuators": list(self._held)})
        self._pending = None
        self._seq += 1

    def close(self) -> None:
        """Release any socket this loop owns.  Stdio streams are left open."""
        self._closed = True
        for resource in self._owned:
            resource.close()
        self._owned.clear()

    def __enter__(self) -> "Loop":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    # -- transport -----------------------------------------------------------

    def _read(self) -> Optional[Dict[str, Any]]:
        line = self._reader.readline()
        if not line:
            return None
        if isinstance(line, bytes):
            line = line.decode("utf-8", errors="replace")
        try:
            frame = json.loads(line)
        except ValueError as e:
            raise ProtocolError(f"malformed frame {line.strip()!r}: {e}") from None
        if not isinstance(frame, dict):
            raise ProtocolError(f"malformed frame {line.strip()!r}: not an object")
        return frame

    def _write(self, frame: Dict[str, Any]) -> None:
        text = json.dumps(frame, separators=(",", ":"), allow_nan=False) + "\n"
        self._writer.write(text if isinstance(self._writer, io.TextIOBase) else text.encode("utf-8"))
        self._writer.flush()

def __getattr__(name):
    # The sampled Loop client is stdlib-only. Environment clients opt into
    # NumPy when they import Gym/Frames; ordinary controllers do not need it.
    if name in ('Frames', 'Gym', 'GymError'):
        from . import gym
        return getattr(gym, name)
    raise AttributeError(name)
