"""POST /nodes/{id}/sketch: curve-index arguments reach the kernel as
curves, whatever their count. `Service.edit_sketch` used to turn every
two-number list into a point before mapping indices, so a join of exactly
two curves, a trim with two cutters or an extend to two targets failed,
and circle_tangent's curves and arc_tangent's `prev` were never mapped.
Headless, through the service the REST route calls, with the JSON call
lists a REST client (the native viewer's `SketchCall::to_json`) sends."""

import pytest

from robocad.api import ApiError, Service
from robocad.document import Document


def _sketch(calls):
    service = Service(Document())
    nid = service.create({"kind": "sketch", "plane": "xy"})["id"]
    return service, nid, service.edit_sketch(nid, calls)["sketch"]["curves"]


def _pt(p, q, tol=1e-6):
    return p[0] == pytest.approx(q[0], abs=tol) and p[1] == pytest.approx(q[1], abs=tol)


def test_join_of_exactly_two_curves():
    _, _, curves = _sketch([["line", [[0, 0], [10, 0]]], ["line", [[10, 0], [10, 10]]], ["join", [[0, 1]]]])
    assert len(curves) == 1
    assert curves[0]["kind"] == "polyline" and not curves[0]["closed"]
    assert curves[0]["points"] == [[0, 0], [10, 0], [10, 10]]


def test_trim_with_two_cutters():
    calls = [
        ["line", [[0, 0], [30, 0]]],
        ["line", [[10, -5], [10, 5]]],
        ["line", [[20, -5], [20, 5]]],
        ["trim", [0, [1, 2], [15, 0]]],
    ]
    _, _, curves = _sketch(calls)
    # The middle of the first line is cut out: two pieces in its place, then the cutters.
    assert [c["kind"] for c in curves] == ["line", "line", "line", "line"]
    assert _pt(curves[0]["points"][0], (0, 0)) and _pt(curves[0]["points"][1], (10, 0))
    assert _pt(curves[1]["points"][0], (20, 0)) and _pt(curves[1]["points"][1], (30, 0))
    assert curves[2]["points"] == [[10, -5], [10, 5]]
    assert curves[3]["points"] == [[20, -5], [20, 5]]


def test_extend_to_two_targets():
    calls = [
        ["line", [[5, 0], [15, 0]]],
        ["line", [[0, -5], [0, 5]]],
        ["line", [[20, -5], [20, 5]]],
        ["extend", [0, [1, 2]]],
    ]
    _, _, curves = _sketch(calls)
    assert len(curves) == 3
    assert _pt(curves[0]["points"][0], (0, 0)) and _pt(curves[0]["points"][1], (20, 0))


def test_circle_tangent_to_three_curves():
    # The 30-40-50 right triangle's incircle: radius (30 + 40 - 50) / 2 = 10 at (10, 10).
    calls = [
        ["line", [[0, 0], [40, 0]]],
        ["line", [[40, 0], [0, 30]]],
        ["line", [[0, 30], [0, 0]]],
        ["circle_tangent", [[0, 1, 2], None, [12, 8]]],
    ]
    _, _, curves = _sketch(calls)
    assert len(curves) == 4
    circle = curves[3]
    assert circle["kind"] == "circle"
    assert _pt(circle["center"], (10, 10), tol=1e-2)
    assert circle["radius"] == pytest.approx(10, abs=1e-2)


def test_arc_tangent_after_a_line():
    # Tangent to +x at (10, 0), ending at (20, 10): a quarter arc of radius 10 about (10, 10).
    _, _, curves = _sketch([["line", [[0, 0], [10, 0]]], ["arc_tangent", [0, [20, 10]]]])
    assert len(curves) == 2
    arc = curves[1]
    assert arc["kind"] == "arc"
    assert _pt(arc["center"], (10, 10))
    assert arc["radius"] == pytest.approx(10)
    assert arc["start_angle"] == pytest.approx(-90)
    assert arc["end_angle"] == pytest.approx(0)


def test_points_still_become_points():
    _, _, curves = _sketch([["rectangle", [[0, 0], [20, 10]]], ["polyline", [[[0, 0], [5, 5]], True]]])
    assert curves[0]["points"] == [[0, 0], [20, 0], [20, 10], [0, 10]] and curves[0]["closed"]
    assert curves[1]["points"] == [[0, 0], [5, 5]] and curves[1]["closed"]


def test_circle_tangent_to_two_curves_with_a_radius():
    # Two perpendicular lines through the origin: the r = 5 circle in the
    # first quadrant touches both at (5, 0) and (0, 5), centred at (5, 5).
    calls = [
        ["line", [[0, 0], [20, 0]]],
        ["line", [[0, 0], [0, 20]]],
        ["circle_tangent", [[0, 1], 5, [4, 6]]],
    ]
    _, _, curves = _sketch(calls)
    assert len(curves) == 3
    circle = curves[2]
    assert circle["kind"] == "circle"
    assert _pt(circle["center"], (5, 5), tol=1e-2)
    assert circle["radius"] == pytest.approx(5)


def test_create_with_a_refused_call_leaves_no_sketch():
    # Collinear points have no circumcircle: the create is refused (422) and
    # neither the empty sketch nor its "Sketch" undo step remains.
    service = Service(Document())
    service.create({"kind": "box", "corner": [0, 0, 0], "size": [1, 1, 1]})
    nodes, history = set(service.doc.nodes), service.history()
    with pytest.raises(ApiError) as refused:
        service.create({"kind": "sketch", "plane": "xy", "calls": [["circle_three_point", [[0, 0], [1, 1], [2, 2]]]]})
    assert refused.value.status == 422
    assert "collinear" in str(refused.value)
    assert set(service.doc.nodes) == nodes
    assert service.history() == history


def test_kernel_refusal_is_422_and_changes_nothing():
    service, nid, _ = _sketch([["circle", [[0, 0], 10]]])
    history = service.history()
    with pytest.raises(ApiError) as refused:
        service.edit_sketch(nid, [["extend", [0, []]]])
    assert refused.value.status == 422
    assert len(service.doc.nodes[nid].sketch.curves) == 1
    assert service.history() == history


def test_out_of_range_curve_index_is_400():
    service, nid, _ = _sketch([["line", [[0, 0], [10, 0]]]])
    # Python's negative indices (from the end) still work; past either end is refused.
    assert service.edit_sketch(nid, [["reverse", [-1]]])["sketch"]["curves"][0]["points"] == [[10, 0], [0, 0]]
    for index in (1, -2):
        with pytest.raises(ApiError) as refused:
            service.edit_sketch(nid, [["reverse", [index]]])
        assert refused.value.status == 400
        assert f"curve index {index} out of range" in str(refused.value)
