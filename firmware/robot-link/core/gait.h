/*
 * gait -- a leg gait plan from the gait pack, and the reference governor
 * that turns it into servo goals, free of any platform.
 *
 * The plan format and every number in it come from the Rust library
 * (crates/sim-runtime/src/hardware/calibration/gait_pack.rs). The governor
 * is sim_domain_control::reference_governor::Config::update, stepped as
 * gait_playback::GovernedGait::step does, in encoder counts; each plan
 * carries a check sequence from the Rust governor, and rl_plan_check()
 * replays it so a divergence shows up as a number instead of as motion.
 * The goal bytes are calibration_serial::servo_command(ServoPosition), and
 * for a multi-turn motor servo_command(ServoSpeed).
 * Change one side, change all of them.
 */
#ifndef GAIT_H
#define GAIT_H
#ifdef __cplusplus
extern "C" {
#endif
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define RL_GAIT_MAX_AXES 3
#define RL_PLAN_AXIS_BYTES 36
enum { RL_DRIVE_POSITION = 0, RL_DRIVE_SPEED = 1 }; /* servo control mode register 0x21 */
#define RL_PLAN_MULTI_TURN 1                        /* plan axis flag */

typedef struct {
    uint8_t id;
    uint8_t drive;                 /* RL_DRIVE_*: position for one turn, speed for more */
    bool    multi_turn;            /* the window spans more than one encoder turn */
    int32_t window_lo, window_hi;  /* taught poses, counts (plus whole turns when multi-turn): the FPGA window */
    float   governor_period_s;
    float   max_speed_counts_s;
    float   max_accel_counts_s2;
    float   response_rate_per_s;
    float   hold_tolerance_counts; /* speed drive: no drive this close to a resting goal */
    int32_t turn_margin_counts;    /* multi-turn: where a confirmed turn may put the motor beyond the window */
} rl_plan_axis_t;

typedef struct {
    float    period_s;
    uint16_t samples;              /* desired samples over one period */
    uint8_t  axes;
    rl_plan_axis_t axis[RL_GAIT_MAX_AXES];
    const uint8_t *desired;        /* samples x axes little-endian f32, in the caller's buffer */
    uint16_t check_steps;
    float    check_dt_s;
    const uint8_t *check;          /* check_steps x axes f32 */
} rl_plan_t;

/* Parse a plan in `b` (kept by the caller for as long as the plan is used). 0 on any inconsistency. */
int rl_plan_parse(const uint8_t *b, size_t n, rl_plan_t *out);
/* Desired counts of plan axis `k` at gait time t (periodic, linear between samples). */
float rl_plan_desired(const rl_plan_t *p, int k, float t);
/* Largest |C governor - Rust governor| over the plan's check sequence, in counts. */
float rl_plan_check(const rl_plan_t *p);

typedef struct { float x, v; } rl_gov_t;
/* Advance dt seconds toward `desired`, as GovernedGait::step: ceil(dt/period) equal sub-steps. */
void rl_gov_step(const rl_plan_axis_t *a, rl_gov_t *s, float desired, float dt);

/* Servo position-mode goal write: register 0x2A, goal (0..4095), time 0, speed limit. 7 bytes. */
void rl_position_goal(float target_counts, float velocity_counts_s, int32_t lo, int32_t hi, uint8_t out[7]);
/* Servo speed-mode write: register 0x2E, the reference speed plus 4/s of position
 * error (counts, turns included), zero once settled within `tolerance` of a goal at
 * rest or when it would push past the window [lo, hi]. 3 bytes. */
void rl_speed_goal(float target_counts, float velocity_counts_s, int32_t position_counts, int32_t lo, int32_t hi, float tolerance, uint8_t out[3]);
#ifdef __cplusplus
}
#endif
#endif
