#include "gait.h"
#include <math.h>
#include <string.h>

static float f32_at(const uint8_t *p)
{
    float v;
    memcpy(&v, p, 4); /* little-endian hosts only (ESP32-P4, x86, arm64) */
    return v;
}
static int32_t i32_at(const uint8_t *p)
{
    int32_t v;
    memcpy(&v, p, 4);
    return v;
}

int rl_plan_parse(const uint8_t *b, size_t n, rl_plan_t *p)
{
    memset(p, 0, sizeof *p);
    if (n < 12 || memcmp(b, "PLAN", 4)) return 0;
    p->period_s = f32_at(b + 4);
    p->samples = (uint16_t)(b[8] | b[9] << 8);
    p->axes = b[10];
    if (!(p->period_s > 0.f) || p->samples < 2 || p->axes < 1 || p->axes > RL_GAIT_MAX_AXES) return 0;
    size_t at = 12;
    if (n < at + (size_t)RL_PLAN_AXIS_BYTES * p->axes) return 0;
    for (int k = 0; k < p->axes; k++, at += RL_PLAN_AXIS_BYTES) {
        rl_plan_axis_t *a = &p->axis[k];
        a->id = b[at];
        a->drive = b[at + 1];
        a->multi_turn = b[at + 2] & RL_PLAN_MULTI_TURN;
        a->window_lo = i32_at(b + at + 4);
        a->window_hi = i32_at(b + at + 8);
        a->governor_period_s = f32_at(b + at + 12);
        a->max_speed_counts_s = f32_at(b + at + 16);
        a->max_accel_counts_s2 = f32_at(b + at + 20);
        a->response_rate_per_s = f32_at(b + at + 24);
        a->hold_tolerance_counts = f32_at(b + at + 28);
        a->turn_margin_counts = i32_at(b + at + 32);
        if (!a->id || a->window_lo >= a->window_hi || (b[at + 2] & ~RL_PLAN_MULTI_TURN) || b[at + 3]) return 0;
        if (a->multi_turn) {
            /* Speed drive only (position mode cannot leave its one turn); a reading
             * must allow at most two places, as the FPGA's window bounds allow. */
            if (a->drive != RL_DRIVE_SPEED || a->window_lo < -8000000 || a->window_hi > 8000000 || a->turn_margin_counts < 0
                || a->turn_margin_counts > 1024 || (int64_t)a->window_hi - a->window_lo + 2 * a->turn_margin_counts >= 2 * 4096) return 0;
        } else if (a->window_lo < 0 || a->window_hi > 4095 || a->drive > RL_DRIVE_SPEED) return 0;
        if (!(a->governor_period_s > 0.f) || !(a->max_speed_counts_s > 0.f) || !(a->max_accel_counts_s2 > 0.f) || !(a->response_rate_per_s > 0.f)
            || !(a->hold_tolerance_counts >= 0.f && a->hold_tolerance_counts <= 200.f)) return 0;
    }
    size_t desired_bytes = (size_t)p->samples * p->axes * 4;
    if (n < at + desired_bytes + 8) return 0;
    p->desired = b + at;
    at += desired_bytes;
    p->check_steps = (uint16_t)(b[at] | b[at + 1] << 8);
    p->check_dt_s = f32_at(b + at + 4);
    at += 8;
    p->check = b + at;
    at += (size_t)p->check_steps * p->axes * 4;
    return at == n;
}

float rl_plan_desired(const rl_plan_t *p, int k, float t)
{
    float u = fmodf(t, p->period_s);
    if (u < 0) u += p->period_s;
    float x = u / p->period_s * p->samples;
    int i = (int)x;
    if (i >= p->samples) i = p->samples - 1;
    int j = (i + 1) % p->samples;
    float f = x - (float)i;
    float a = f32_at(p->desired + 4 * ((size_t)i * p->axes + k));
    float b = f32_at(p->desired + 4 * ((size_t)j * p->axes + k));
    return a + (b - a) * f;
}

static float clampf(float v, float lo, float hi)
{
    return v < lo ? lo : v > hi ? hi : v;
}

void rl_gov_step(const rl_plan_axis_t *a, rl_gov_t *s, float desired, float dt)
{
    if (!(dt > 0.f)) return;
    int n = (int)ceilf(dt / a->governor_period_s);
    if (n < 1) n = 1;
    float h = dt / (float)n, w = a->response_rate_per_s, amax = a->max_accel_counts_s2, vmax = a->max_speed_counts_s;
    for (int i = 0; i < n; i++) {
        float acceleration = clampf(w * w * (desired - s->x) - 2.f * w * s->v, -amax, amax);
        float wanted = clampf(s->v + h * acceleration, -vmax, vmax);
        float dv = h * amax;
        float v = clampf(wanted, s->v - dv, s->v + dv);
        s->x += h * v;
        s->v = v;
    }
}

float rl_plan_check(const rl_plan_t *p)
{
    rl_gov_t g[RL_GAIT_MAX_AXES];
    for (int k = 0; k < p->axes; k++) g[k] = (rl_gov_t){rl_plan_desired(p, k, 0.f), 0.f};
    float worst = 0.f;
    for (int s = 0; s < p->check_steps; s++) {
        for (int k = 0; k < p->axes; k++) {
            rl_gov_step(&p->axis[k], &g[k], rl_plan_desired(p, k, s * p->check_dt_s), p->check_dt_s);
            float e = fabsf(g[k].x - f32_at(p->check + 4 * ((size_t)s * p->axes + k)));
            if (e > worst) worst = e;
        }
    }
    return worst;
}

void rl_position_goal(float target, float velocity, int32_t lo, int32_t hi, uint8_t out[7])
{
    long g = lroundf(target);
    if (g < lo) g = lo;
    if (g > hi) g = hi;
    g = ((g % 4096) + 4096) % 4096;
    float speed = clampf(fabsf(velocity) * 1.3f + 150.f, 150.f, 3000.f);
    uint16_t sp = (uint16_t)speed;
    out[0] = 0x2A;
    out[1] = (uint8_t)g;
    out[2] = (uint8_t)(g >> 8);
    out[3] = 0;
    out[4] = 0;
    out[5] = (uint8_t)sp;
    out[6] = (uint8_t)(sp >> 8);
}

void rl_speed_goal(float target, float velocity, int32_t position, int32_t lo, int32_t hi, float tolerance, uint8_t out[3])
{
    float error = target - (float)position;
    bool settled = fabsf(velocity) < 1.f && fabsf(error) <= tolerance;
    float speed = settled ? 0.f : clampf(velocity + 4.f * error, -3000.f, 3000.f);
    /* Never ask for motion further out past a window edge (the FPGA would drop it). */
    if ((speed > 0.f && position >= hi) || (speed < 0.f && position <= lo)) speed = 0.f;
    long mag = lroundf(fabsf(speed));
    if (mag > 0x7fff) mag = 0x7fff;
    uint16_t raw = (uint16_t)mag | (speed < 0.f ? 0x8000 : 0);
    out[0] = 0x2E;
    out[1] = (uint8_t)raw;
    out[2] = (uint8_t)(raw >> 8);
}
