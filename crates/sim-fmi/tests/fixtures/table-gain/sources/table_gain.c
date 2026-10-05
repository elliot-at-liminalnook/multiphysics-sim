/*
 * Test fixture: y = gain * u, with the gain read from resources/gain.txt
 * at every step (not once at instantiation), so the importer must keep the
 * extracted resources for the instance's whole life.
 */
#include <stdio.h>
#include <string.h>
#include "fmi3Functions.h"

#define MODEL_TOKEN "{table-gain-fixture-1}"
#define MODEL_USES_RESOURCES

enum { VR_U = 0, VR_Y = 1 };

typedef struct {
    double u, y;
    char gain_file[2100];
} ModelState;

static void model_defaults(ModelState *m) {
    m->u = 0.0;
    m->y = 0.0;
    m->gain_file[0] = 0;
}

static void model_resources(ModelState *m, const char *resource_path) {
    snprintf(m->gain_file, sizeof m->gain_file, "%sgain.txt", resource_path);
}

static fmi3Status model_get(const ModelState *m, fmi3ValueReference vr, double *v) {
    switch (vr) {
    case VR_U: *v = m->u; break;
    case VR_Y: *v = m->y; break;
    default: return fmi3Error;
    }
    return fmi3OK;
}

static fmi3Status model_set(ModelState *m, fmi3ValueReference vr, double v, int initialized) {
    (void)initialized;
    if (vr != VR_U) return fmi3Error;
    m->u = v;
    return fmi3OK;
}

static void model_initialize(ModelState *m, double t) {
    (void)t;
    m->y = 0.0;
}

static fmi3Status model_step(ModelState *m, double t, double h, const char **why) {
    (void)t; (void)h;
    double gain;
    FILE *f = fopen(m->gain_file, "r");
    if (!f) { *why = "resources/gain.txt cannot be opened"; return fmi3Error; }
    int read = fscanf(f, "%lf", &gain);
    fclose(f);
    if (read != 1) { *why = "resources/gain.txt holds no number"; return fmi3Error; }
    m->y = gain * m->u;
    return fmi3OK;
}

#include "cosim_scaffold.h"
