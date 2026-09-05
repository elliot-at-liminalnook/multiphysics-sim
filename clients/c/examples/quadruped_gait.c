/* A trot for the planar quadruped, in C, over the seam's stdio protocol.
 *
 *     cc -O2 -std=c99 -Wall -Wextra -Werror -I.. -o quadruped_gait quadruped_gait.c -lm
 *     ./quadruped_gait --stride=0.12 --period=0.6 --height=0.478 --start=0.5
 *
 * The same law as clients/python/examples/quadruped_gait.py, flag for flag:
 * diagonal pairs (fl+rr, fr+rl) alternate; a stance foot sweeps backward
 * under its hip, a swing foot returns along an arc; two-link inverse
 * kinematics turns foot targets into joint targets and PD into torques.
 * Sensors: <leg>.<joint>.angle and .speed; actuators: <leg>.<joint>.torque.
 */
#define SIMLOOP_IMPLEMENTATION
#include "simloop.h"

#include <math.h>
#include <stdlib.h>
#include <string.h>

static const char *LEGS[4] = {"fl", "fr", "rl", "rr"};
static const double PHASE[4] = {0.0, 0.5, 0.5, 0.0};

typedef struct {
    double stride, period, duty, lift, height, l1, l2, kp, kd, start;
} gait_t;

static void inverse(double x, double y, double l1, double l2, double *hip, double *knee) {
    double r2 = x * x + y * y;
    double c = (r2 - l1 * l1 - l2 * l2) / (2.0 * l1 * l2);
    if (c > 1.0) c = 1.0;
    if (c < -1.0) c = -1.0;
    *knee = -acos(c);
    *hip = atan2(y, x) - atan2(l2 * sin(*knee), l1 + l2 * cos(*knee));
}

static void foot_target(const gait_t *g, double phase, double *x, double *y) {
    if (phase < g->duty) {
        double s = phase / g->duty;
        *x = g->stride * (0.5 - s);
        *y = -g->height;
    } else {
        double s = (phase - g->duty) / (1.0 - g->duty);
        *x = g->stride * (s - 0.5);
        *y = -g->height + g->lift * sin(M_PI * s);
    }
}

static int flag(const char *arg, const char *name, double *out) {
    size_t n = strlen(name);
    if (strncmp(arg, name, n) == 0 && arg[n] == '=') {
        *out = atof(arg + n + 1);
        return 1;
    }
    return 0;
}

int main(int argc, char **argv) {
    gait_t g = {0.12, 0.6, 0.5, 0.05, 0.478, 0.25, 0.25, 60.0, 2.0, 0.5};
    simloop_t loop;
    simloop_frame_t f;
    double act[SIMLOOP_MAX_CHANNELS] = {0};
    double previous[4][2];
    int have_previous[4] = {0, 0, 0, 0};
    int angle[4][2], speed[4][2], torque[4][2];
    int i, j, r;

    for (i = 1; i < argc; i++) {
        if (!(flag(argv[i], "--stride", &g.stride) || flag(argv[i], "--period", &g.period) || flag(argv[i], "--duty", &g.duty)
              || flag(argv[i], "--lift", &g.lift) || flag(argv[i], "--height", &g.height) || flag(argv[i], "--l1", &g.l1)
              || flag(argv[i], "--l2", &g.l2) || flag(argv[i], "--kp", &g.kp) || flag(argv[i], "--kd", &g.kd) || flag(argv[i], "--start", &g.start))) {
            fprintf(stderr, "quadruped_gait: unknown argument %s\n", argv[i]);
            return 2;
        }
    }
    if (simloop_open(&loop, stdin, stdout) != 0) {
        fprintf(stderr, "quadruped_gait: %s\n", loop.error);
        return 1;
    }
    for (i = 0; i < 4; i++) {
        const char *joints[2] = {"hip", "knee"};
        for (j = 0; j < 2; j++) {
            char name[64];
            snprintf(name, sizeof name, "%s.%s.angle", LEGS[i], joints[j]);
            angle[i][j] = simloop_sensor_index(&loop, name);
            snprintf(name, sizeof name, "%s.%s.speed", LEGS[i], joints[j]);
            speed[i][j] = simloop_sensor_index(&loop, name);
            snprintf(name, sizeof name, "%s.%s.torque", LEGS[i], joints[j]);
            torque[i][j] = simloop_actuator_index(&loop, name);
            if (angle[i][j] < 0 || speed[i][j] < 0 || torque[i][j] < 0) {
                fprintf(stderr, "quadruped_gait: channel %s missing\n", name);
                return 1;
            }
        }
    }
    fprintf(stderr, "quadruped_gait (C): %s period=%.17g stride=%.17g\n", loop.element, loop.period, g.stride);

    while ((r = simloop_next(&loop, &f)) == 1) {
        for (i = 0; i < 4; i++) {
            double tx, ty, hip, knee, target[2], rate[2];
            if (f.t < g.start) {
                tx = 0.0;
                ty = -g.height;
            } else {
                double phase = fmod((f.t - g.start) / g.period + PHASE[i], 1.0);
                foot_target(&g, phase, &tx, &ty);
            }
            inverse(tx, ty, g.l1, g.l2, &hip, &knee);
            target[0] = hip;
            target[1] = knee;
            for (j = 0; j < 2; j++) {
                double last = have_previous[i] ? previous[i][j] : target[j];
                rate[j] = (target[j] - last) / loop.period;
                previous[i][j] = target[j];
                act[torque[i][j]] = g.kp * (target[j] - f.sensors[angle[i][j]]) + g.kd * (rate[j] - f.sensors[speed[i][j]]);
            }
            have_previous[i] = 1;
        }
        if (simloop_send(&loop, &f, act) != 0) break;
    }
    if (r < 0 || loop.error[0]) {
        fprintf(stderr, "quadruped_gait: %s\n", loop.error);
        return 1;
    }
    return 0;
}
