/*
 * simloop.h -- single-header C99 client for the simulator's external-controller
 * seam, over stdin/stdout.  No dependencies beyond the C library.
 *
 *     #define SIMLOOP_IMPLEMENTATION      (in exactly one translation unit)
 *     #include "simloop.h"
 *
 *     simloop_t loop;
 *     if (simloop_open(&loop, stdin, stdout) != 0) return 1;   // reads hello, sends ready
 *     simloop_frame_t f; double act[SIMLOOP_MAX_CHANNELS] = {0};
 *     while (simloop_next(&loop, &f) == 1) {                    // 1 frame, 0 closed, -1 error
 *         act[0] = -2.0 * f.sensors[0];
 *         simloop_send(&loop, &f, act);
 *     }
 *
 * The protocol is newline-delimited JSON in lockstep; the simulator speaks
 * first (hello), the client answers ready, then each sample gets an act with
 * the same seq and exactly loop.n_actuators values.  f.t is simulation time;
 * there is no wall clock here.  Nothing but frames may go to `out`; log to
 * stderr.  Doubles are printed with %.17g so they round-trip exactly.
 */
#ifndef SIMLOOP_H
#define SIMLOOP_H

#include <stdio.h>

#define SIMLOOP_MAX_CHANNELS 64
#define SIMLOOP_NAME_MAX 64      /* names and units, including the terminator */
#define SIMLOOP_LINE_MAX 16384

typedef struct {
    FILE *in, *out;
    char element[SIMLOOP_NAME_MAX];
    double period;
    int n_sensors, n_actuators;
    char sensor_names[SIMLOOP_MAX_CHANNELS][SIMLOOP_NAME_MAX];
    char sensor_units[SIMLOOP_MAX_CHANNELS][SIMLOOP_NAME_MAX];
    char actuator_names[SIMLOOP_MAX_CHANNELS][SIMLOOP_NAME_MAX];
    char actuator_units[SIMLOOP_MAX_CHANNELS][SIMLOOP_NAME_MAX];
    char error[256];              /* set when a call returns -1 */
    /* private */
    unsigned long long seq;       /* next expected sample seq */
    int pending;                  /* a frame awaits its act */
    double held[SIMLOOP_MAX_CHANNELS];
    char line[SIMLOOP_LINE_MAX];
} simloop_t;

typedef struct {
    unsigned long long seq;
    double t;                     /* simulation time */
    int n_sensors;
    double sensors[SIMLOOP_MAX_CHANNELS];
} simloop_frame_t;

/* Read the hello from `in`, fill the contract fields, write ready to `out`.  0 or -1. */
int simloop_open(simloop_t *loop, FILE *in, FILE *out);
/* Wait for the next sample.  1 = frame in *f, 0 = close frame or end of stream, -1 = error.
 * If the previous frame was never answered, its held actuator values are sent first. */
int simloop_next(simloop_t *loop, simloop_frame_t *f);
/* Answer frame f with act[0..n_actuators).  0 or -1. */
int simloop_send(simloop_t *loop, const simloop_frame_t *f, const double *act);
/* Channel lookup by name; -1 if absent. */
int simloop_sensor_index(const simloop_t *loop, const char *name);
int simloop_actuator_index(const simloop_t *loop, const char *name);

#endif /* SIMLOOP_H */

#ifdef SIMLOOP_IMPLEMENTATION

#include <errno.h>
#include <stdlib.h>
#include <string.h>

/* ---- a scanner just big enough for this protocol's JSON ------------------ */

static const char *sl_ws(const char *p) {
    while (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r') p++;
    return p;
}

static char *sl_put_utf8(char *o, char *end, unsigned cp) {
    if (cp < 0x80) { if (o < end) *o++ = (char)cp; }
    else if (cp < 0x800) { if (o + 1 < end) { *o++ = (char)(0xC0 | (cp >> 6)); *o++ = (char)(0x80 | (cp & 0x3F)); } else o = end + 1; }
    else if (cp < 0x10000) { if (o + 2 < end) { *o++ = (char)(0xE0 | (cp >> 12)); *o++ = (char)(0x80 | ((cp >> 6) & 0x3F)); *o++ = (char)(0x80 | (cp & 0x3F)); } else o = end + 1; }
    else { if (o + 3 < end) { *o++ = (char)(0xF0 | (cp >> 18)); *o++ = (char)(0x80 | ((cp >> 12) & 0x3F)); *o++ = (char)(0x80 | ((cp >> 6) & 0x3F)); *o++ = (char)(0x80 | (cp & 0x3F)); } else o = end + 1; }
    return o;
}

/* Parse the string at p (which must be '"') into out (cap bytes, NUL-terminated).
 * Returns the position after the closing quote, or NULL if malformed or too long. */
static const char *sl_string(const char *p, char *out, size_t cap) {
    char *o = out, *end = out + cap - 1;
    if (*p != '"') return NULL;
    for (p++; *p && *p != '"'; p++) {
        if (o > end) return NULL;
        if (*p != '\\') { if (o < end) *o++ = *p; else return NULL; continue; }
        switch (*++p) {
        case '"': case '\\': case '/': if (o < end) *o++ = *p; else return NULL; break;
        case 'n': if (o < end) *o++ = '\n'; else return NULL; break;
        case 't': if (o < end) *o++ = '\t'; else return NULL; break;
        case 'r': if (o < end) *o++ = '\r'; else return NULL; break;
        case 'b': if (o < end) *o++ = '\b'; else return NULL; break;
        case 'f': if (o < end) *o++ = '\f'; else return NULL; break;
        case 'u': {
            char hex[5]; char *e; unsigned cp;
            if (strlen(p + 1) < 4) return NULL;
            memcpy(hex, p + 1, 4); hex[4] = 0;
            cp = (unsigned)strtoul(hex, &e, 16);
            if (e != hex + 4) return NULL;
            p += 4;
            if (cp >= 0xD800 && cp < 0xDC00 && p[1] == '\\' && p[2] == 'u' && strlen(p + 3) >= 4) {
                unsigned lo; memcpy(hex, p + 3, 4);
                lo = (unsigned)strtoul(hex, &e, 16);
                if (e == hex + 4 && lo >= 0xDC00 && lo < 0xE000) { cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00); p += 6; }
            }
            o = sl_put_utf8(o, end, cp);
            if (o > end) return NULL;
            break;
        }
        default: return NULL;
        }
    }
    if (*p != '"') return NULL;
    *o = 0;
    return p + 1;
}

/* Skip one JSON value of any kind.  NULL if malformed. */
static const char *sl_skip(const char *p) {
    p = sl_ws(p);
    if (*p == '"') {
        for (p++; *p && *p != '"'; p++) if (*p == '\\' && p[1]) p++;
        return *p == '"' ? p + 1 : NULL;
    }
    if (*p == '[' || *p == '{') {
        char close = *p == '[' ? ']' : '}';
        p = sl_ws(p + 1);
        if (*p == close) return p + 1;
        for (;;) {
            if (close == '}') {
                if (!(p = sl_skip(p))) return NULL;           /* key */
                if (*(p = sl_ws(p)) != ':') return NULL;
                p++;
            }
            if (!(p = sl_skip(p))) return NULL;
            p = sl_ws(p);
            if (*p == close) return p + 1;
            if (*p != ',') return NULL;
            p = sl_ws(p + 1);
        }
    }
    if (!strncmp(p, "true", 4)) return p + 4;
    if (!strncmp(p, "false", 5)) return p + 5;
    if (!strncmp(p, "null", 4)) return p + 4;
    {
        char *e;
        strtod(p, &e);
        return e > p ? e : NULL;
    }
}

/* In the object at p, find `key` and return a pointer to its value, else NULL. */
static const char *sl_find(const char *p, const char *key) {
    char k[SIMLOOP_NAME_MAX];
    p = sl_ws(p);
    if (*p != '{') return NULL;
    p = sl_ws(p + 1);
    if (*p == '}') return NULL;
    for (;;) {
        const char *after = sl_string(p, k, sizeof k);
        int match = after != NULL && !strcmp(k, key);
        if (!after) { if (!(after = sl_skip(p))) return NULL; }  /* an over-long key we don't want */
        if (*(after = sl_ws(after)) != ':') return NULL;
        after = sl_ws(after + 1);
        if (match) return after;
        if (!(p = sl_skip(after))) return NULL;
        p = sl_ws(p);
        if (*p == '}') return NULL;
        if (*p != ',') return NULL;
        p = sl_ws(p + 1);
    }
}

static int sl_number(const char *p, double *out) {
    char *e;
    if (!p) return 0;
    *out = strtod(p, &e);
    return e > p;
}

static int sl_fail(simloop_t *loop, const char *what) {
    snprintf(loop->error, sizeof loop->error, "%s in %.120s", what, loop->line);
    return -1;
}

/* Read one line into loop->line.  1 = line, 0 = end of stream, -1 = error. */
static int sl_read(simloop_t *loop) {
    size_t n;
    if (!fgets(loop->line, SIMLOOP_LINE_MAX, loop->in)) {
        loop->line[0] = 0;
        if (ferror(loop->in)) { snprintf(loop->error, sizeof loop->error, "read failed: %s", strerror(errno)); return -1; }
        return 0;
    }
    n = strlen(loop->line);
    if (n && loop->line[n - 1] == '\n') loop->line[--n] = 0;
    else if (n == SIMLOOP_LINE_MAX - 1) { snprintf(loop->error, sizeof loop->error, "frame longer than %d bytes", SIMLOOP_LINE_MAX); return -1; }
    return 1;
}

/* Does the frame in loop->line have "type":<type>? */
static int sl_type_is(simloop_t *loop, const char *type, char *got, size_t cap) {
    const char *v = sl_find(loop->line, "type");
    if (!v || !sl_string(v, got, cap)) { got[0] = 0; return 0; }
    return !strcmp(got, type);
}

static int sl_channels(simloop_t *loop, const char *key, char names[][SIMLOOP_NAME_MAX], char units[][SIMLOOP_NAME_MAX], int *count) {
    const char *p = sl_find(loop->line, key);
    char what[80];
    snprintf(what, sizeof what, "bad or missing \"%s\"", key);
    *count = 0;
    if (!p || *p != '[') return sl_fail(loop, what);
    p = sl_ws(p + 1);
    if (*p == ']') return 0;
    for (;;) {
        const char *name, *unit;
        if (*count == SIMLOOP_MAX_CHANNELS) return sl_fail(loop, "more channels than SIMLOOP_MAX_CHANNELS");
        name = sl_find(p, "name");
        unit = sl_find(p, "unit");
        if (!name || !unit || !sl_string(name, names[*count], SIMLOOP_NAME_MAX) || !sl_string(unit, units[*count], SIMLOOP_NAME_MAX)) return sl_fail(loop, what);
        (*count)++;
        if (!(p = sl_skip(p))) return sl_fail(loop, what);
        p = sl_ws(p);
        if (*p == ']') return 0;
        if (*p != ',') return sl_fail(loop, what);
        p = sl_ws(p + 1);
    }
}

static int sl_write_act(simloop_t *loop, unsigned long long seq) {
    int i;
    if (fprintf(loop->out, "{\"type\":\"act\",\"seq\":%llu,\"actuators\":[", seq) < 0) goto fail;
    for (i = 0; i < loop->n_actuators; i++)
        if (fprintf(loop->out, i ? ",%.17g" : "%.17g", loop->held[i]) < 0) goto fail;
    if (fputs("]}\n", loop->out) == EOF || fflush(loop->out) == EOF) goto fail;
    return 0;
fail:
    snprintf(loop->error, sizeof loop->error, "write failed: %s", strerror(errno));
    return -1;
}

/* ---- public API ------------------------------------------------------------ */

int simloop_open(simloop_t *loop, FILE *in, FILE *out) {
    const char *v;
    char type[16];
    int r;
    memset(loop, 0, sizeof *loop);
    loop->in = in;
    loop->out = out;
    if ((r = sl_read(loop)) <= 0) return r == 0 ? sl_fail(loop, "stream closed before hello") : -1;
    if (!sl_type_is(loop, "hello", type, sizeof type)) return sl_fail(loop, "expected hello");
    v = sl_find(loop->line, "element");
    if (!v || !sl_string(v, loop->element, sizeof loop->element)) return sl_fail(loop, "bad or missing \"element\"");
    if (!sl_number(sl_find(loop->line, "period"), &loop->period)) return sl_fail(loop, "bad or missing \"period\"");
    if (sl_channels(loop, "sensors", loop->sensor_names, loop->sensor_units, &loop->n_sensors) != 0) return -1;
    if (sl_channels(loop, "actuators", loop->actuator_names, loop->actuator_units, &loop->n_actuators) != 0) return -1;
    if (fputs("{\"type\":\"ready\"}\n", loop->out) == EOF || fflush(loop->out) == EOF) {
        snprintf(loop->error, sizeof loop->error, "write failed: %s", strerror(errno));
        return -1;
    }
    return 0;
}

int simloop_next(simloop_t *loop, simloop_frame_t *f) {
    const char *p;
    char *e, type[16];
    int r;
    if (loop->pending) {
        if (sl_write_act(loop, loop->seq) != 0) return -1;
        loop->pending = 0;
        loop->seq++;
    }
    if ((r = sl_read(loop)) <= 0) return r;
    if (sl_type_is(loop, "close", type, sizeof type)) return 0;
    if (strcmp(type, "sample") != 0) return sl_fail(loop, "unexpected frame");
    p = sl_find(loop->line, "seq");
    if (!p || *p < '0' || *p > '9') return sl_fail(loop, "bad or missing \"seq\"");
    f->seq = strtoull(p, &e, 10);
    if (f->seq != loop->seq) { snprintf(loop->error, sizeof loop->error, "expected seq %llu, got %llu", loop->seq, f->seq); return -1; }
    if (!sl_number(sl_find(loop->line, "t"), &f->t)) return sl_fail(loop, "bad or missing \"t\"");
    p = sl_find(loop->line, "sensors");
    if (!p || *p != '[') return sl_fail(loop, "bad or missing \"sensors\"");
    p = sl_ws(p + 1);
    f->n_sensors = 0;
    while (*p != ']') {
        if (f->n_sensors == SIMLOOP_MAX_CHANNELS || !sl_number(p, &f->sensors[f->n_sensors])) return sl_fail(loop, "bad \"sensors\"");
        f->n_sensors++;
        strtod(p, &e);
        p = sl_ws(e);
        if (*p == ',') p = sl_ws(p + 1);
        else if (*p != ']') return sl_fail(loop, "bad \"sensors\"");
    }
    if (f->n_sensors != loop->n_sensors) { snprintf(loop->error, sizeof loop->error, "expected %d sensors, got %d", loop->n_sensors, f->n_sensors); return -1; }
    loop->pending = 1;
    return 1;
}

int simloop_send(simloop_t *loop, const simloop_frame_t *f, const double *act) {
    int i;
    if (!loop->pending || f->seq != loop->seq) { snprintf(loop->error, sizeof loop->error, "no frame with seq %llu awaits a reply", f->seq); return -1; }
    for (i = 0; i < loop->n_actuators; i++) {
        if (act[i] != act[i] || act[i] - act[i] != 0) { snprintf(loop->error, sizeof loop->error, "actuator %d is not finite", i); return -1; }
        loop->held[i] = act[i];
    }
    if (sl_write_act(loop, f->seq) != 0) return -1;
    loop->pending = 0;
    loop->seq++;
    return 0;
}

static int sl_index(const char names[][SIMLOOP_NAME_MAX], int n, const char *name) {
    int i;
    for (i = 0; i < n; i++) if (!strcmp(names[i], name)) return i;
    return -1;
}

int simloop_sensor_index(const simloop_t *loop, const char *name) { return sl_index(loop->sensor_names, loop->n_sensors, name); }
int simloop_actuator_index(const simloop_t *loop, const char *name) { return sl_index(loop->actuator_names, loop->n_actuators, name); }

#endif /* SIMLOOP_IMPLEMENTATION */
