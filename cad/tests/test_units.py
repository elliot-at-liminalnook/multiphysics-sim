import math

import pytest

from robocad.units import ExpressionError, evaluate, format_angle, format_length


def test_bare_number_is_millimetres():
    assert evaluate("20") == 20.0


def test_unit_suffixes():
    assert evaluate("20mm + 0.3") == pytest.approx(20.3)
    assert evaluate("1in") == pytest.approx(25.4)
    assert evaluate("2cm") == pytest.approx(20.0)
    assert evaluate("1m") == pytest.approx(1000.0)
    assert evaluate("1ft") == pytest.approx(304.8)
    assert evaluate('(1/2)"') == pytest.approx(12.7)


def test_arithmetic_and_pi():
    assert evaluate("50/2") == 25.0
    assert evaluate("pi*10") == pytest.approx(math.pi * 10)
    assert evaluate("(1in + 2mm) * 2") == pytest.approx(54.8)
    assert evaluate("2^3") == 8.0
    assert evaluate("-5 + 10") == 5.0
    assert evaluate("sqrt(16)") == 4.0


def test_angles():
    assert evaluate("45deg", angle=True) == 45.0
    assert evaluate("pi rad", angle=True) == pytest.approx(180.0)
    assert evaluate("0.5turn", angle=True) == pytest.approx(180.0)


def test_default_unit_for_bare_numbers():
    assert evaluate("2", default_unit="in") == pytest.approx(50.8)
    assert evaluate("2 + 1mm", default_unit="in") == pytest.approx(51.8)


def test_errors():
    with pytest.raises(ExpressionError):
        evaluate("")
    with pytest.raises(ExpressionError):
        evaluate("2 +")
    with pytest.raises(ExpressionError):
        evaluate("1/0")
    with pytest.raises(ExpressionError):
        evaluate("__import__('os')")
    with pytest.raises(ExpressionError):
        evaluate("30deg")  # an angle in a length field


def test_formatting():
    assert format_length(25.4, "in") == "1 in"
    assert format_length(12.345) == "12.345 mm"
    assert format_angle(90.0) == "90°"
