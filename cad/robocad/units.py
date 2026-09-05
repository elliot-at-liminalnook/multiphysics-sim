"""Numeric entry: millimetres internally, unit suffixes, arithmetic and `pi`.

    >>> evaluate("20mm + 0.3")
    20.3
    >>> evaluate("1in")
    25.4
    >>> evaluate("pi*10")
    31.415926535897931
    >>> evaluate("45deg", angle=True)
    45.0

Lengths resolve to millimetres; angles to degrees. A bare number takes the
default unit of the field (`mm` or `deg`). The grammar is a small
recursive-descent parser — no `eval` — so a typed expression can never do
anything but arithmetic.
"""

from __future__ import annotations

import math
import re
from dataclasses import dataclass
from typing import Optional

LENGTH_UNITS = {
    "mm": 1.0,
    "millimeter": 1.0,
    "millimetre": 1.0,
    "cm": 10.0,
    "m": 1000.0,
    "in": 25.4,
    "inch": 25.4,
    '"': 25.4,
    "ft": 304.8,
    "'": 304.8,
    "thou": 0.0254,
    "mil": 0.0254,
    "um": 0.001,
    "µm": 0.001,
}
ANGLE_UNITS = {"deg": 1.0, "°": 1.0, "rad": 180.0 / math.pi, "grad": 0.9, "turn": 360.0}
CONSTANTS = {"pi": math.pi, "e": math.e, "tau": math.tau}
FUNCTIONS = {"sin": lambda x: math.sin(math.radians(x)), "cos": lambda x: math.cos(math.radians(x)), "tan": lambda x: math.tan(math.radians(x)), "sqrt": math.sqrt, "abs": abs, "round": round, "floor": math.floor, "ceil": math.ceil}

_TOKEN = re.compile(r"\s*(?:(\d+\.\d*|\.\d+|\d+)(?:[eE][-+]?\d+)?|([A-Za-z_µ°\"']+)|(\*\*|[-+*/^%(),]))")


class ExpressionError(ValueError):
    pass


@dataclass
class _Tok:
    kind: str  # 'num', 'name', 'op'
    text: str


def _tokenize(text: str) -> list[_Tok]:
    out: list[_Tok] = []
    pos = 0
    text = text.strip()
    while pos < len(text):
        m = _TOKEN.match(text, pos)
        if not m or m.end() == pos:
            raise ExpressionError(f"unexpected character {text[pos]!r} at {pos}")
        num, name, op = m.groups()
        if num is not None:
            out.append(_Tok("num", m.group(0).strip()))
        elif name is not None:
            out.append(_Tok("name", name))
        else:
            out.append(_Tok("op", op))
        pos = m.end()
    return out


class _Parser:
    """expr := term (('+'|'-') term)* ; term := unary (('*'|'/'|'%') unary)* ;
    unary := ('-'|'+') unary | power ; power := atom ('^' unary)? ;
    atom := number unit? | name '(' expr ')' | name | '(' expr ')' unit?"""

    def __init__(self, tokens: list[_Tok], angle: bool, default_unit: Optional[str]):
        self.t = tokens
        self.i = 0
        self.angle = angle
        self.units = ANGLE_UNITS if angle else LENGTH_UNITS
        self.default = default_unit
        self.saw_unit = False

    def peek(self) -> Optional[_Tok]:
        return self.t[self.i] if self.i < len(self.t) else None

    def take(self, kind=None, text=None) -> _Tok:
        tok = self.peek()
        if tok is None or (kind and tok.kind != kind) or (text and tok.text != text):
            raise ExpressionError(f"expected {text or kind} at token {self.i}")
        self.i += 1
        return tok

    def expr(self) -> float:
        v = self.term()
        while (tok := self.peek()) and tok.kind == "op" and tok.text in "+-":
            self.i += 1
            w = self.term()
            v = v + w if tok.text == "+" else v - w
        return v

    def term(self) -> float:
        v = self.unary()
        while (tok := self.peek()) and tok.kind == "op" and tok.text in ("*", "/", "%"):
            self.i += 1
            w = self.unary()
            if tok.text == "*":
                v *= w
            elif tok.text == "/":
                if w == 0:
                    raise ExpressionError("division by zero")
                v /= w
            else:
                v %= w
        return v

    def unary(self) -> float:
        tok = self.peek()
        if tok and tok.kind == "op" and tok.text in "+-":
            self.i += 1
            v = self.unary()
            return -v if tok.text == "-" else v
        return self.power()

    def power(self) -> float:
        v = self.atom()
        tok = self.peek()
        if tok and tok.kind == "op" and tok.text in ("^", "**"):
            self.i += 1
            v = v ** self.unary()
        return v

    def unit_suffix(self) -> Optional[float]:
        tok = self.peek()
        if tok and tok.kind == "name":
            key = tok.text
            if key in self.units:
                self.i += 1
                self.saw_unit = True
                return self.units[key]
            # A length unit in an angle field (or the reverse) is an error worth naming.
            other = ANGLE_UNITS if not self.angle else LENGTH_UNITS
            if key in other:
                raise ExpressionError(f"{key!r} is not a {'angle' if self.angle else 'length'} unit")
        return None

    def atom(self) -> float:
        tok = self.peek()
        if tok is None:
            raise ExpressionError("unexpected end of expression")
        if tok.kind == "num":
            self.i += 1
            v = float(tok.text)
            scale = self.unit_suffix()
            return v * (scale if scale is not None else self.default_scale())
        if tok.kind == "name":
            self.i += 1
            name = tok.text
            if name in FUNCTIONS:
                self.take("op", "(")
                arg = self.expr()
                self.take("op", ")")
                return FUNCTIONS[name](arg)
            if name in CONSTANTS:
                v = CONSTANTS[name]
                scale = self.unit_suffix()
                return v * (scale if scale is not None else self.default_scale())
            raise ExpressionError(f"unknown name {name!r}")
        if tok.kind == "op" and tok.text == "(":
            self.i += 1
            v = self.expr()
            self.take("op", ")")
            scale = self.unit_suffix()
            return v * scale if scale is not None else v
        raise ExpressionError(f"unexpected {tok.text!r}")

    def default_scale(self) -> float:
        if self.default is None:
            return 1.0
        return self.units[self.default]


def evaluate(text: str, angle: bool = False, default_unit: Optional[str] = None) -> float:
    """Evaluate `text` to millimetres (or degrees when `angle`). Bare numbers
    take `default_unit` (a key of the unit table) or the internal unit."""
    tokens = _tokenize(text)
    if not tokens:
        raise ExpressionError("empty expression")
    p = _Parser(tokens, angle, default_unit)
    v = p.expr()
    if p.peek() is not None:
        raise ExpressionError(f"trailing input at token {p.i}")
    if math.isnan(v) or math.isinf(v):
        raise ExpressionError("result is not a finite number")
    return v


def try_evaluate(text: str, angle: bool = False, default_unit: Optional[str] = None) -> Optional[float]:
    try:
        return evaluate(text, angle, default_unit)
    except ExpressionError:
        return None


def format_length(mm: float, unit: str = "mm", digits: int = 3) -> str:
    scale = LENGTH_UNITS[unit]
    v = mm / scale
    s = f"{v:.{digits}f}".rstrip("0").rstrip(".")
    return f"{s or '0'} {unit}"


def format_angle(deg: float, digits: int = 2) -> str:
    s = f"{deg:.{digits}f}".rstrip("0").rstrip(".")
    return f"{s or '0'}°"
