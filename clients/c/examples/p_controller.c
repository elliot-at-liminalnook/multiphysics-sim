/* A proportional controller over stdio: actuator i = -gain * sensor i.
 *
 *     cc -std=c99 -Wall -Wextra -Werror -I.. -o p_controller p_controller.c
 *     ./p_controller [gain]        (default gain 2)
 */
#define SIMLOOP_IMPLEMENTATION
#include "simloop.h"

#include <stdlib.h>

int main(int argc, char **argv) {
    double gain = argc > 1 ? atof(argv[1]) : 2.0;
    simloop_t loop;
    simloop_frame_t f;
    double act[SIMLOOP_MAX_CHANNELS] = {0};
    int i, r;

    if (simloop_open(&loop, stdin, stdout) != 0) {
        fprintf(stderr, "p_controller: %s\n", loop.error);
        return 1;
    }
    fprintf(stderr, "p_controller: element=%s period=%.17g gain=%.17g\n", loop.element, loop.period, gain);
    for (i = 0; i < loop.n_sensors; i++) fprintf(stderr, "  sensor   %s [%s]\n", loop.sensor_names[i], loop.sensor_units[i]);
    for (i = 0; i < loop.n_actuators; i++) fprintf(stderr, "  actuator %s [%s]\n", loop.actuator_names[i], loop.actuator_units[i]);

    while ((r = simloop_next(&loop, &f)) == 1) {
        for (i = 0; i < loop.n_actuators; i++) act[i] = i < f.n_sensors ? -gain * f.sensors[i] : 0.0;
        if (simloop_send(&loop, &f, act) != 0) break;
    }
    if (r < 0 || loop.error[0]) {
        fprintf(stderr, "p_controller: %s\n", loop.error);
        return 1;
    }
    return 0;
}
