/*
 * A behavioural model of the supervised bridge (hx_safety.v + bridge.v) and
 * its servos: the bench profile (IDs 4..12, no windows) or, with
 * fb_init_calibration, the leg's calibration profile (IDs 1..3, taught
 * windows anchored to fresh feedback and counted in turns, 400/600 ms
 * leases, position goals only inside the window, speed goals never outward
 * past it, version 8), in
 * milliseconds. It exists so the link engine's decisions can be tested on
 * any machine in a few milliseconds; the real RTL is the reference, and
 * cosim/ runs the engine against that.
 */
#ifndef FAKE_BRIDGE_H
#define FAKE_BRIDGE_H
#include "../core/hx.h"
#include <stdbool.h>

#define FB_COUNT 9
#define FB_FIRST 4

typedef struct {
    bool     present;
    uint8_t  mode, torque, lock;
    float    position_f;          /* the shaft in counts, turns included: toward goal in position mode, at speed_goal in speed mode */
    uint16_t goal, goal_speed;
    int16_t  speed_goal;          /* speed mode (1), counts/s */
    int16_t  speed;
    uint16_t pwm_raw;
    uint16_t position;
    uint8_t  voltage_raw, temperature_c, error;
    uint32_t pwm_writes, torque_writes, goal_writes, mode_writes, speed_writes;
} fb_servo_t;

typedef struct {
    hx_framer_t framer;
    uint8_t  first, count;        /* the profile's IDs */
    bool     calibration;
    bool     window_valid[FB_COUNT];
    int32_t  lower[FB_COUNT], upper[FB_COUNT];
    /* The supervisor's own turn count, from the readings it forwards (hx_safety.v continuous_position). */
    int32_t  continuous[FB_COUNT];
    uint16_t sampled[FB_COUNT];
    bool     continuous_valid[FB_COUNT];
    uint32_t feedback_timeout_ms, command_timeout_ms;
    bool     latched;
    uint8_t  reason, fault_id;
    uint16_t armed;
    uint32_t feedback_age_ms[FB_COUNT], command_age_ms[FB_COUNT];
    bool     seen[FB_COUNT], healthy[FB_COUNT];
    bool     s2;
    fb_servo_t servo[FB_COUNT];
    /* Bytes waiting to go to the host, with the time they become visible. */
    uint8_t  out[512];
    size_t   out_len;
    uint32_t out_ready_ms;
    uint32_t stops, refused, forwarded, local;
    uint32_t now_ms;
} fb_t;

void fb_init(fb_t *fb, uint16_t present_mask);
/* The calibration profile: IDs 1..3, present_mask bit 0 = ID 1. */
void fb_init_calibration(fb_t *fb, uint16_t present_mask);
/* Advance the clock (timeouts age). */
void fb_tick(fb_t *fb, uint32_t now_ms);
/* Bytes from the host. */
void fb_rx(fb_t *fb, const uint8_t *bytes, size_t n);
/* Bytes to the host that are ready at now_ms; returns the count copied. */
size_t fb_tx(fb_t *fb, uint8_t *out, size_t cap);
#endif
