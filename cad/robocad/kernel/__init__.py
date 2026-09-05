from .base import (  # noqa: F401
    Body,
    BooleanOp,
    ChamferSpec,
    CurveKind,
    EdgeRef,
    FaceRef,
    GeometryKernel,
    KernelError,
    MassProperties,
    Mesh,
    Plane,
    SurfaceKind,
    SweepOptions,
    ValidationIssue,
    ValidationReport,
    Vec3,
    VertexRef,
)


def __getattr__(name):
    # Basic value types and errors do not require OCCT. Keep headless cached
    # experiments cheap; load the CAD sketch implementation only on demand.
    if name in ('Curve', 'Sketch'):
        from .sketch import Curve, Sketch
        globals().update(Curve=Curve, Sketch=Sketch)
        return globals()[name]
    raise AttributeError(name)


def default_kernel() -> GeometryKernel:
    from .occt import OcctKernel

    return OcctKernel()
