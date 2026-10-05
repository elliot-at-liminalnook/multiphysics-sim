/*
 * Thermostat: a hysteresis (bang-bang) temperature controller as an FMI 3
 * Co-Simulation FMU. The heater is switched on below setpoint - band/2 and
 * off above setpoint + band/2; in between it keeps its state. Over a
 * communication step [t, t+h) the input temperature is held, and the
 * outputs at t+h are the heater state decided from it.
 *
 * fail_after / nan_after (s, negative = never) make the FMU fail its step
 * or output NaN from that time on: fixtures for the importer's fault
 * handling.
 */
#include <math.h>
#include "fmi3Functions.h"

#define MODEL_TOKEN "{6f3e1d2a-0c4b-4e7f-9a51-thermostat-1}"

enum { VR_TEMPERATURE = 0, VR_HEATER_POWER = 1, VR_HEATING = 2,
       VR_SETPOINT = 10, VR_BAND = 11, VR_RATED_POWER = 12, VR_FAIL_AFTER = 13, VR_NAN_AFTER = 14,
       VR_SWITCHES = 20 };

typedef struct {
    double temperature;  /* input, K */
    double setpoint, band, rated_power, fail_after, nan_after; /* parameters */
    int heating;         /* state and output */
    double heater_power; /* output, W */
    double switches;     /* local: how many times the heater switched */
} ModelState;

static void model_defaults(ModelState *m) {
    m->temperature = 293.15;
    m->setpoint = 294.15;
    m->band = 1.0;
    m->rated_power = 500.0;
    m->fail_after = -1.0;
    m->nan_after = -1.0;
    m->heating = 0;
    m->heater_power = 0.0;
    m->switches = 0.0;
}

static fmi3Status model_get(const ModelState *m, fmi3ValueReference vr, double *v) {
    switch (vr) {
    case VR_TEMPERATURE: *v = m->temperature; break;
    case VR_HEATER_POWER: *v = m->heater_power; break;
    case VR_HEATING: *v = m->heating; break;
    case VR_SETPOINT: *v = m->setpoint; break;
    case VR_BAND: *v = m->band; break;
    case VR_RATED_POWER: *v = m->rated_power; break;
    case VR_FAIL_AFTER: *v = m->fail_after; break;
    case VR_NAN_AFTER: *v = m->nan_after; break;
    case VR_SWITCHES: *v = m->switches; break;
    default: return fmi3Error;
    }
    return fmi3OK;
}

static fmi3Status model_set(ModelState *m, fmi3ValueReference vr, double v, int initialized) {
    if (vr == VR_TEMPERATURE) { m->temperature = v; return fmi3OK; }
    if (initialized) return fmi3Error; /* parameters are fixed after instantiation */
    switch (vr) {
    case VR_SETPOINT: m->setpoint = v; break;
    case VR_BAND: if (v < 0) return fmi3Error; m->band = v; break;
    case VR_RATED_POWER: if (v < 0) return fmi3Error; m->rated_power = v; break;
    case VR_FAIL_AFTER: m->fail_after = v; break;
    case VR_NAN_AFTER: m->nan_after = v; break;
    default: return fmi3Error;
    }
    return fmi3OK;
}

static void decide(ModelState *m) {
    int was = m->heating;
    if (m->temperature < m->setpoint - 0.5 * m->band) m->heating = 1;
    else if (m->temperature > m->setpoint + 0.5 * m->band) m->heating = 0;
    if (m->heating != was) m->switches += 1.0;
    m->heater_power = m->heating ? m->rated_power : 0.0;
}

static void model_initialize(ModelState *m, double t) {
    (void)t;
    decide(m);
}

static fmi3Status model_step(ModelState *m, double t, double h, const char **why) {
    if (m->fail_after >= 0 && t + h > m->fail_after) {
        *why = "deliberate failure (fail_after)";
        return fmi3Error;
    }
    decide(m);
    if (m->nan_after >= 0 && t + h > m->nan_after) m->heater_power = NAN;
    return fmi3OK;
}

#include "cosim_scaffold.h"
