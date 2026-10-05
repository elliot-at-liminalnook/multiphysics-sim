/*
 * A minimal FMI 3.0 Co-Simulation scaffold for hand-written models (the
 * repository's fixture FMUs). Part of multiphysics-sim, MIT licensed.
 *
 * A model defines, before including this header:
 *
 *   typedef struct { ... } ModelState;          its variables
 *   static void model_defaults(ModelState *m);  start values
 *   static fmi3Status model_get(const ModelState *m, fmi3ValueReference vr, double *v);
 *   static fmi3Status model_set(ModelState *m, fmi3ValueReference vr, double v, int initialized);
 *   static void model_initialize(ModelState *m, double t);          initial outputs
 *   static fmi3Status model_step(ModelState *m, double t, double h, const char **why);
 *
 * Every variable is exchanged as Float64 or Boolean (model_get/model_set
 * see doubles; Booleans are 0/1). All state lives in the instance: no
 * globals, so instances are independent. FMU state save/restore copies the
 * instance. Event mode, clocks, Model Exchange and Scheduled Execution are
 * not offered (their entry points report an error).
 */
#ifndef COSIM_SCAFFOLD_H
#define COSIM_SCAFFOLD_H

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "fmi3Functions.h"

enum { MODE_INSTANTIATED, MODE_INITIALIZATION, MODE_STEP, MODE_TERMINATED };

typedef struct {
    fmi3InstanceEnvironment environment;
    fmi3LogMessageCallback log;
    int logging;
    int mode;
    double time;
    ModelState m;
} Instance;

static void say(Instance *c, fmi3Status status, const char *category, const char *message) {
    if (c->log && (c->logging || status != fmi3OK)) c->log(c->environment, status, category, message);
}

const char *fmi3GetVersion(void) { return fmi3Version; }

fmi3Status fmi3SetDebugLogging(fmi3Instance instance, fmi3Boolean on, size_t n, const fmi3String categories[]) {
    (void)n; (void)categories;
    ((Instance *)instance)->logging = on;
    return fmi3OK;
}

fmi3Instance fmi3InstantiateCoSimulation(fmi3String name, fmi3String token, fmi3String resources, fmi3Boolean visible,
                                         fmi3Boolean logging, fmi3Boolean eventModeUsed, fmi3Boolean earlyReturnAllowed,
                                         const fmi3ValueReference required[], size_t nRequired,
                                         fmi3InstanceEnvironment environment, fmi3LogMessageCallback log,
                                         fmi3IntermediateUpdateCallback intermediate) {
    (void)name; (void)resources; (void)visible; (void)earlyReturnAllowed; (void)required; (void)intermediate;
    if (strcmp(token, MODEL_TOKEN) != 0) {
        if (log) log(environment, fmi3Error, "error", "instantiation token does not match this binary");
        return NULL;
    }
    if (eventModeUsed || nRequired > 0) {
        if (log) log(environment, fmi3Error, "error", "event mode and intermediate update are not supported");
        return NULL;
    }
    Instance *c = calloc(1, sizeof(Instance));
    if (!c) return NULL;
    c->environment = environment;
    c->log = log;
    c->logging = logging;
    c->mode = MODE_INSTANTIATED;
    model_defaults(&c->m);
    return c;
}

fmi3Instance fmi3InstantiateModelExchange(fmi3String a, fmi3String b, fmi3String c, fmi3Boolean d, fmi3Boolean e,
                                          fmi3InstanceEnvironment env, fmi3LogMessageCallback log) {
    (void)a; (void)b; (void)c; (void)d; (void)e;
    if (log) log(env, fmi3Error, "error", "Model Exchange is not offered");
    return NULL;
}

void fmi3FreeInstance(fmi3Instance instance) { free(instance); }

fmi3Status fmi3EnterInitializationMode(fmi3Instance instance, fmi3Boolean toleranceDefined, fmi3Float64 tolerance,
                                       fmi3Float64 startTime, fmi3Boolean stopTimeDefined, fmi3Float64 stopTime) {
    (void)toleranceDefined; (void)tolerance; (void)stopTimeDefined; (void)stopTime;
    Instance *c = instance;
    if (c->mode != MODE_INSTANTIATED) { say(c, fmi3Error, "error", "fmi3EnterInitializationMode outside Instantiated"); return fmi3Error; }
    c->time = startTime;
    c->mode = MODE_INITIALIZATION;
    return fmi3OK;
}

fmi3Status fmi3ExitInitializationMode(fmi3Instance instance) {
    Instance *c = instance;
    if (c->mode != MODE_INITIALIZATION) { say(c, fmi3Error, "error", "fmi3ExitInitializationMode outside Initialization Mode"); return fmi3Error; }
    model_initialize(&c->m, c->time);
    c->mode = MODE_STEP;
    return fmi3OK;
}

fmi3Status fmi3EnterEventMode(fmi3Instance instance) { say(instance, fmi3Error, "error", "event mode is not offered"); return fmi3Error; }

fmi3Status fmi3Terminate(fmi3Instance instance) { ((Instance *)instance)->mode = MODE_TERMINATED; return fmi3OK; }

fmi3Status fmi3Reset(fmi3Instance instance) {
    Instance *c = instance;
    c->mode = MODE_INSTANTIATED;
    c->time = 0;
    model_defaults(&c->m);
    return fmi3OK;
}

fmi3Status fmi3GetFloat64(fmi3Instance instance, const fmi3ValueReference vr[], size_t nvr, fmi3Float64 values[], size_t nValues) {
    Instance *c = instance;
    if (nvr != nValues) return fmi3Error;
    for (size_t i = 0; i < nvr; i++)
        if (model_get(&c->m, vr[i], &values[i]) != fmi3OK) { say(c, fmi3Error, "error", "fmi3GetFloat64: unknown value reference"); return fmi3Error; }
    return fmi3OK;
}

fmi3Status fmi3SetFloat64(fmi3Instance instance, const fmi3ValueReference vr[], size_t nvr, const fmi3Float64 values[], size_t nValues) {
    Instance *c = instance;
    if (nvr != nValues) return fmi3Error;
    for (size_t i = 0; i < nvr; i++)
        if (model_set(&c->m, vr[i], values[i], c->mode != MODE_INSTANTIATED) != fmi3OK) { say(c, fmi3Error, "error", "fmi3SetFloat64: not settable now"); return fmi3Error; }
    return fmi3OK;
}

fmi3Status fmi3GetBoolean(fmi3Instance instance, const fmi3ValueReference vr[], size_t nvr, fmi3Boolean values[], size_t nValues) {
    Instance *c = instance;
    if (nvr != nValues) return fmi3Error;
    for (size_t i = 0; i < nvr; i++) {
        double v;
        if (model_get(&c->m, vr[i], &v) != fmi3OK) { say(c, fmi3Error, "error", "fmi3GetBoolean: unknown value reference"); return fmi3Error; }
        values[i] = v != 0.0;
    }
    return fmi3OK;
}

fmi3Status fmi3SetBoolean(fmi3Instance instance, const fmi3ValueReference vr[], size_t nvr, const fmi3Boolean values[], size_t nValues) {
    Instance *c = instance;
    if (nvr != nValues) return fmi3Error;
    for (size_t i = 0; i < nvr; i++)
        if (model_set(&c->m, vr[i], values[i] ? 1.0 : 0.0, c->mode != MODE_INSTANTIATED) != fmi3OK) return fmi3Error;
    return fmi3OK;
}

fmi3Status fmi3DoStep(fmi3Instance instance, fmi3Float64 t, fmi3Float64 h, fmi3Boolean noSetFMUStatePriorToCurrentPoint,
                      fmi3Boolean *eventHandlingNeeded, fmi3Boolean *terminateSimulation, fmi3Boolean *earlyReturn,
                      fmi3Float64 *lastSuccessfulTime) {
    (void)noSetFMUStatePriorToCurrentPoint;
    Instance *c = instance;
    *eventHandlingNeeded = fmi3False;
    *terminateSimulation = fmi3False;
    *earlyReturn = fmi3False;
    *lastSuccessfulTime = c->time;
    if (c->mode != MODE_STEP) { say(c, fmi3Error, "error", "fmi3DoStep outside Step Mode"); return fmi3Error; }
    if (h <= 0) { say(c, fmi3Error, "error", "fmi3DoStep with a non-positive step"); return fmi3Error; }
    const char *why = NULL;
    fmi3Status status = model_step(&c->m, t, h, &why);
    if (status != fmi3OK) {
        say(c, status, "error", why ? why : "the model step failed");
        return status;
    }
    c->time = t + h;
    *lastSuccessfulTime = c->time;
    return fmi3OK;
}

/* FMU state: a copy of the instance. */
fmi3Status fmi3GetFMUState(fmi3Instance instance, fmi3FMUState *state) {
    Instance *copy = *state ? *state : malloc(sizeof(Instance));
    if (!copy) return fmi3Error;
    memcpy(copy, instance, sizeof(Instance));
    *state = copy;
    return fmi3OK;
}

fmi3Status fmi3SetFMUState(fmi3Instance instance, fmi3FMUState state) {
    Instance *c = instance, *saved = state;
    fmi3InstanceEnvironment environment = c->environment;
    fmi3LogMessageCallback log = c->log;
    memcpy(c, saved, sizeof(Instance));
    c->environment = environment;
    c->log = log;
    return fmi3OK;
}

fmi3Status fmi3FreeFMUState(fmi3Instance instance, fmi3FMUState *state) {
    (void)instance;
    free(*state);
    *state = NULL;
    return fmi3OK;
}

fmi3Status fmi3SerializedFMUStateSize(fmi3Instance instance, fmi3FMUState state, size_t *size) {
    (void)instance; (void)state;
    *size = sizeof(Instance);
    return fmi3OK;
}

fmi3Status fmi3SerializeFMUState(fmi3Instance instance, fmi3FMUState state, fmi3Byte bytes[], size_t size) {
    (void)instance;
    if (size != sizeof(Instance)) return fmi3Error;
    memcpy(bytes, state, size);
    return fmi3OK;
}

fmi3Status fmi3DeserializeFMUState(fmi3Instance instance, const fmi3Byte bytes[], size_t size, fmi3FMUState *state) {
    if (size != sizeof(Instance)) { say(instance, fmi3Error, "error", "serialized state has the wrong size"); return fmi3Error; }
    Instance *copy = *state ? *state : malloc(sizeof(Instance));
    if (!copy) return fmi3Error;
    memcpy(copy, bytes, size);
    *state = copy;
    return fmi3OK;
}

#endif
