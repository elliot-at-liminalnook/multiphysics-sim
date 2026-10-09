/*
 * Joint controller: the outer loop of one robot joint as an FMI 3
 * Co-Simulation FMU. It generates a reference r(t) = offset + setpoint +
 * amplitude · sin(2π·frequency·t) and commands the joint's servo target
 * with integral trim for steady error. `setpoint` is an input a person can
 * drive (teleoperation's setpoint for this joint; 0 when nothing drives it):
 *
 *   target(t+h) = clamp(r(t+h) + i, -limit, limit),  i += ki·(r(t) − angle(t))·h
 *
 * Over a step [t, t+h) the measured angle is held. Several instances run
 * side by side (one per joint), each with its own state.
 */
#include <math.h>
#include "fmi3Functions.h"

#define MODEL_TOKEN "{0d7c2b44-9e1a-4c35-b8f0-joint-controller-1}"

enum { VR_ANGLE = 0, VR_TARGET = 1, VR_SETPOINT = 2, VR_AMPLITUDE = 10, VR_FREQUENCY = 11, VR_OFFSET = 12, VR_KI = 13, VR_LIMIT = 14, VR_TRIM = 20 };

typedef struct {
    double angle;                                   /* input, rad */
    double setpoint;                                /* input, rad */
    double amplitude, frequency, offset, ki, limit; /* parameters */
    double trim;                                    /* integral state, rad */
    double target;                                  /* output, rad */
} ModelState;

static const double TWO_PI = 6.283185307179586;

static void model_defaults(ModelState *m) {
    m->angle = 0.0;
    m->setpoint = 0.0;
    m->amplitude = 0.3;
    m->frequency = 0.5;
    m->offset = 0.0;
    m->ki = 0.0;
    m->limit = 1.5;
    m->trim = 0.0;
    m->target = 0.0;
}

static fmi3Status model_get(const ModelState *m, fmi3ValueReference vr, double *v) {
    switch (vr) {
    case VR_ANGLE: *v = m->angle; break;
    case VR_TARGET: *v = m->target; break;
    case VR_SETPOINT: *v = m->setpoint; break;
    case VR_AMPLITUDE: *v = m->amplitude; break;
    case VR_FREQUENCY: *v = m->frequency; break;
    case VR_OFFSET: *v = m->offset; break;
    case VR_KI: *v = m->ki; break;
    case VR_LIMIT: *v = m->limit; break;
    case VR_TRIM: *v = m->trim; break;
    default: return fmi3Error;
    }
    return fmi3OK;
}

static fmi3Status model_set(ModelState *m, fmi3ValueReference vr, double v, int initialized) {
    if (vr == VR_ANGLE) { m->angle = v; return fmi3OK; }
    if (vr == VR_SETPOINT) { m->setpoint = v; return fmi3OK; }
    if (initialized) return fmi3Error;
    switch (vr) {
    case VR_AMPLITUDE: m->amplitude = v; break;
    case VR_FREQUENCY: if (v < 0) return fmi3Error; m->frequency = v; break;
    case VR_OFFSET: m->offset = v; break;
    case VR_KI: if (v < 0) return fmi3Error; m->ki = v; break;
    case VR_LIMIT: if (v <= 0) return fmi3Error; m->limit = v; break;
    default: return fmi3Error;
    }
    return fmi3OK;
}

static double reference(const ModelState *m, double t) {
    return m->offset + m->setpoint + m->amplitude * sin(TWO_PI * m->frequency * t);
}

static double clamp(double x, double limit) {
    return x < -limit ? -limit : (x > limit ? limit : x);
}

static void model_initialize(ModelState *m, double t) {
    m->target = clamp(reference(m, t) + m->trim, m->limit);
}

static fmi3Status model_step(ModelState *m, double t, double h, const char **why) {
    if (!isfinite(m->angle)) {
        *why = "the measured angle is not finite";
        return fmi3Error;
    }
    if (!isfinite(m->setpoint)) {
        *why = "the setpoint is not finite";
        return fmi3Error;
    }
    m->trim = clamp(m->trim + m->ki * (reference(m, t) - m->angle) * h, m->limit);
    m->target = clamp(reference(m, t + h) + m->trim, m->limit);
    return fmi3OK;
}

#include "cosim_scaffold.h"
