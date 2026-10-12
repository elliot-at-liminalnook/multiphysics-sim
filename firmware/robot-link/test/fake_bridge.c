#include "fake_bridge.h"
#include <math.h>
#include <string.h>

enum { FEEDBACK_TIMEOUT = 200, COMMAND_TIMEOUT = 300, SERVO_TURNAROUND_MS = 2 };

void fb_init(fb_t *fb, uint16_t present)
{
    memset(fb, 0, sizeof *fb);
    hx_framer_init(&fb->framer);
    fb->first = FB_FIRST;
    fb->count = 9;
    fb->version = 1;
    fb->feedback_timeout_ms = FEEDBACK_TIMEOUT;
    fb->command_timeout_ms = COMMAND_TIMEOUT;
    fb->latched = true;
    fb->reason = 1;
    fb->fault_id = 254;
    for (int i = 0; i < FB_COUNT + FB_SPARES; i++) {
        if (i < FB_COUNT) {
            fb->feedback_age_ms[i] = FEEDBACK_TIMEOUT;
            fb->command_age_ms[i] = COMMAND_TIMEOUT;
        }
        fb_servo_t *s = &fb->servo[i];
        s->present = i < FB_COUNT && (present >> i & 1);
        s->id = (uint8_t)(FB_FIRST + i);
        s->mode = 2;
        s->position = (uint16_t)(1000 + 300 * i);
        s->position_f = s->position;
        s->goal = s->position;
        s->lock = 1;
        s->voltage_raw = 118;
        s->temperature_c = 35;
    }
}

void fb_init_calibration(fb_t *fb, uint16_t present)
{
    fb_init(fb, present);
    fb->first = 1;
    fb->count = 3;
    fb->calibration = true;
    fb->version = 8;
    fb->feedback_timeout_ms = 400;
    fb->command_timeout_ms = 600;
    for (int i = 0; i < FB_COUNT; i++) fb->servo[i].id = (uint8_t)(1 + i);
}

void fb_init_robot(fb_t *fb, uint16_t present)
{
    fb_init_calibration(fb, present);
    fb->count = 16;
    fb->version = 9;
}

fb_servo_t *fb_add_spare(fb_t *fb, uint8_t id)
{
    for (int i = FB_COUNT; i < FB_COUNT + FB_SPARES; i++) {
        fb_servo_t *s = &fb->servo[i];
        if (s->present) continue;
        *s = fb->servo[0];
        s->present = true;
        s->id = id;
        s->position_f = 2000.f;
        s->torque = 0;
        return s;
    }
    return NULL;
}

static fb_servo_t *find_servo(fb_t *fb, uint8_t id)
{
    for (int i = 0; i < FB_COUNT + FB_SPARES; i++)
        if (fb->servo[i].present && fb->servo[i].id == id) return &fb->servo[i];
    return NULL;
}

static void trip(fb_t *fb, uint8_t reason, uint8_t id)
{
    fb->latched = true;
    fb->reason = reason;
    fb->fault_id = id;
    fb->armed = 0;
    fb->stops++;
    for (int i = 0; i < FB_COUNT + FB_SPARES; i++) {
        fb->servo[i].pwm_raw = 0;
        fb->servo[i].torque = 0;
    }
}

void fb_tick(fb_t *fb, uint32_t now)
{
    uint32_t dt = now - fb->now_ms;
    fb->now_ms = now;
    if (fb->permit && (fb->latched == false || (int32_t)(now - fb->permit_until_ms) >= 0)) fb->permit = false;
    for (int i = 0; i < FB_COUNT + FB_SPARES; i++) {
        if (i < FB_COUNT && fb->feedback_age_ms[i] < fb->feedback_timeout_ms) fb->feedback_age_ms[i] += dt;
        if (i < FB_COUNT && fb->command_age_ms[i] < fb->command_timeout_ms) fb->command_age_ms[i] += dt;
        /* Position mode: the shaft moves toward the goal at the goal's speed. */
        fb_servo_t *s = &fb->servo[i];
        float before = s->position_f;
        if (s->torque && s->mode == 0) {
            /* Position mode knows one turn: it moves straight to the goal within 0..4095, never across zero. */
            float within = s->position_f - 4096.f * floorf(s->position_f / 4096.f);
            float step = (float)s->goal_speed * (float)dt / 1000.f, d = (float)s->goal - within;
            s->position_f += d > step ? step : d < -step ? -step : d;
        }
        if (s->torque && s->mode == 1) s->position_f += (float)s->speed_goal * (float)dt / 1000.f;
        if (s->torque && s->mode == 2) {
            /* Open-loop PWM: about 3 counts/s per unit of drive, bit 10 = decreasing. */
            int duty = (int)(s->pwm_raw & 0x3ff) * ((s->pwm_raw >> HX_PWM_DIRECTION_BIT & 1) ? -1 : 1);
            s->position_f += 3.f * (float)duty * (float)dt / 1000.f;
        }
        long whole = lroundf(s->position_f);
        s->position = (uint16_t)(((whole % 4096) + 4096) % 4096);
        s->speed = (int16_t)((s->position_f - before) * 1000.f / (float)(dt ? dt : 1));
    }
    if (fb->s2 && !fb->latched) trip(fb, 9, 254);
    if (fb->latched) return;
    for (int i = 0; i < FB_COUNT; i++) {
        if (!(fb->armed >> i & 1)) continue;
        if (!fb->healthy[i]) { trip(fb, 6, (uint8_t)(fb->first + i)); return; }
        if (fb->feedback_age_ms[i] >= fb->feedback_timeout_ms) { trip(fb, 7, (uint8_t)(fb->first + i)); return; }
        if (fb->command_age_ms[i] >= fb->command_timeout_ms) { trip(fb, 8, (uint8_t)(fb->first + i)); return; }
        /* Leaving the window does not trip (hx_safety.v): it only blocks drive further outward. */
    }
}

static bool fresh(const fb_t *fb, int i)
{
    return fb->seen[i] && fb->healthy[i] && fb->feedback_age_ms[i] < fb->feedback_timeout_ms;
}

static void emit(fb_t *fb, const uint8_t *bytes, size_t n, uint32_t delay_ms)
{
    if (fb->out_len + n > sizeof fb->out) return;
    memcpy(fb->out + fb->out_len, bytes, n);
    fb->out_len += n;
    if (fb->out_len == n) fb->out_ready_ms = fb->now_ms + delay_ms;
}

static void reply(fb_t *fb, uint8_t id, uint8_t error, const uint8_t *params, size_t n, uint32_t delay_ms)
{
    uint8_t frame[HX_MAX_FRAME];
    size_t len = hx_packet(frame, id, error, params, n);
    emit(fb, frame, len, delay_ms);
}

static void status_reply(fb_t *fb)
{
    uint16_t fresh_mask = 0;
    for (int i = 0; i < FB_COUNT; i++) fresh_mask |= (uint16_t)(fresh(fb, i) << i);
    uint8_t p[HX_STATUS_LEN] = {fb->version, fb->latched, fb->reason, fb->fault_id, (uint8_t)fb->armed, (uint8_t)(fb->armed >> 8), (uint8_t)fresh_mask, (uint8_t)(fresh_mask >> 8), 60, 90, 126, 0xD0, 0x07};
    fb->local++;
    reply(fb, HX_BRIDGE_ID, 0, p, HX_STATUS_LEN, 0);
}

static int axis_of(const fb_t *fb, uint8_t id)
{
    int a = (int)id - fb->first;
    return a >= 0 && a < fb->count ? a : -1;
}
static int32_t nearest_turn(int32_t to, int32_t from)
{
    return ((to - from) % 4096 + 4096 + 2048) % 4096 - 2048;
}
static int32_t le32(const uint8_t *p)
{
    return (int32_t)((uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24);
}

static void local_command(fb_t *fb, const uint8_t *p, size_t n)
{
    int a = n >= 2 ? axis_of(fb, p[1]) : -1;
    switch (p[0]) {
    case 7: /* calibration window [7,id,anchor,lower,upper]: only disarmed, anchored to fresh feedback */
        if (fb->calibration && n == 14 && a >= 0 && !(fb->armed >> a & 1)) {
            int32_t anchor = le32(p + 2), lo = le32(p + 6), hi = le32(p + 10);
            /* The anchor is the supervisor's new turn count; its low 12 bits must be the last reading. */
            fb->window_valid[a] = fresh(fb, a) && (anchor & 4095) == fb->sampled[a] && lo < hi && lo <= anchor && anchor <= hi;
            if (fb->window_valid[a]) { fb->lower[a] = lo; fb->upper[a] = hi; fb->continuous[a] = anchor; fb->continuous_valid[a] = true; }
        }
        break;
    case HX_OP_STOP: if (n == 1) trip(fb, 10, 254); break;
    case HX_OP_ARM:
        if (n == 2 && a >= 0 && fresh(fb, a) && (!fb->calibration || (!(fb->armed >> a & 1) && fb->window_valid[a]
            && fb->continuous[a] >= fb->lower[a] && fb->continuous[a] <= fb->upper[a]))) {
            fb->latched = false;
            fb->reason = 0;
            fb->fault_id = 254;
            fb->armed |= (uint16_t)(1u << a);
            fb->command_age_ms[a] = 0;
        }
        break;
    case HX_OP_HEARTBEAT: if (n == 2 && a >= 0 && !fb->latched && (fb->armed >> a & 1)) fb->command_age_ms[a] = 0; break;
    case HX_OP_HEARTBEAT_MASK: {
        uint16_t mask = n == 3 ? (uint16_t)(p[1] | p[2] << 8) : 0;
        if (mask && mask < (1u << fb->count) && (mask & fb->armed) == mask && !fb->latched)
            for (int i = 0; i < FB_COUNT; i++) if (mask >> i & 1) fb->command_age_ms[i] = 0;
        break;
    }
    case HX_OP_DISARM: if (n == 2 && a >= 0) trip(fb, 10, p[1]); break;
    case 9: /* robot profile: [9, old, new], a one-second re-addressing permit, only latched */
        if (fb->version == 9 && n == 3 && fb->latched && p[1] < 254 && p[2] < 254 && p[1] != p[2]) {
            fb->permit = true;
            fb->permit_old = p[1];
            fb->permit_new = p[2];
            fb->permit_until_ms = fb->now_ms + 1000;
        }
        break;
    default: break;
    }
    status_reply(fb);
}

static void forward(fb_t *fb, const uint8_t *f, size_t len)
{
    uint8_t id = f[2], inst = f[4];
    const uint8_t *p = f + 5;
    size_t n = len - 6;
    int a = axis_of(fb, id);
    fb_servo_t *s = find_servo(fb, id);
    if (!s) { fb->refused++; return; } /* nobody answers */
    /* The permit's lock and ID writes, as hx_safety.v READDRESS. */
    if (fb->permit && fb->latched && inst == HX_WRITE && n == 2
        && ((p[0] == 0x37 && (id == fb->permit_old || id == fb->permit_new) && p[1] <= 1) || (p[0] == 0x05 && id == fb->permit_old && p[1] == fb->permit_new))) {
        fb->forwarded++;
        if (p[0] == 0x37) s->lock = p[1];
        if (p[0] == 0x05 && !s->lock) s->id = p[1];
        reply(fb, s->id, 0, NULL, 0, SERVO_TURNAROUND_MS);
        return;
    }
    if (a < 0 && inst != HX_PING && inst != HX_READ) { fb->refused++; return; }
    if (inst == HX_PING) {
        fb->forwarded++;
        reply(fb, id, s->error, NULL, 0, SERVO_TURNAROUND_MS);
        return;
    }
    bool zero = (n == 2 && p[1] == 0) || (n == 3 && p[1] == 0 && p[2] == 0);
    bool allow = inst == HX_PING || inst == HX_READ;
    if (inst == HX_WRITE && n >= 2) {
        uint8_t addr = p[0];
        allow = (addr == HX_REG_TORQUE && n == 2 && zero) || (addr == HX_REG_PWM && n == 3 && zero) || (!fb->latched && (fb->armed >> a & 1));
        if (fb->calibration) {
            if (addr == HX_REG_PWM) {
                /* Bounded drive, never outward past the window (hx_safety.v's calibration rule). */
                uint16_t v = n == 3 ? (uint16_t)(p[1] | p[2] << 8) : 0xffff;
                bool decreasing = v >> HX_PWM_DIRECTION_BIT & 1;
                if (n != 3 || (v >> 11) || (v & 0x3ff) > 1000) allow = false;
                else if (v & 0x3ff) allow = allow && fb->window_valid[a]
                    && !(decreasing && fb->continuous[a] <= fb->lower[a]) && !(!decreasing && fb->continuous[a] >= fb->upper[a]);
            }
            else if (addr == HX_REG_TORQUE || addr == 0x37) allow = allow && n == 2 && p[1] <= 1;
            else if (addr == HX_REG_MODE) allow = allow && n == 2 && p[1] <= 2;
            else if (addr == 0x2A) {
                /* The goal as the nearest turn from the last reading. */
                int32_t goal = fb->continuous[a] + nearest_turn(p[1] | p[2] << 8, fb->sampled[a]);
                allow = allow && (n == 3 || n == 7) && fb->window_valid[a] && goal >= fb->lower[a] && goal <= fb->upper[a];
            } else if (addr == 0x2E) {
                /* Speed, bit 15 = decreasing: never outward past the window. */
                uint16_t v = n == 3 ? (uint16_t)(p[1] | p[2] << 8) : 0;
                if (n != 3) allow = false;
                else if (v & 0x7fff) allow = allow && fb->window_valid[a]
                    && !((v & 0x8000) && fb->continuous[a] <= fb->lower[a]) && !(!(v & 0x8000) && fb->continuous[a] >= fb->upper[a]);
            } else allow = false;
        }
    }
    if (!allow) { fb->refused++; return; }
    fb->forwarded++;
    if (inst == HX_READ && n == 2) {
        uint8_t addr = p[0], width = p[1];
        uint8_t regs[HX_TELEMETRY_LEN] = {0};
        if (addr == HX_REG_TELEMETRY && width == HX_TELEMETRY_LEN) {
            regs[0] = (uint8_t)s->position; regs[1] = (uint8_t)(s->position >> 8);
            uint16_t sp = (uint16_t)(s->speed < 0 ? (-s->speed) | 0x8000 : s->speed);
            regs[2] = (uint8_t)sp; regs[3] = (uint8_t)(sp >> 8);
            regs[6] = s->voltage_raw; regs[7] = s->temperature_c; regs[9] = s->error;
            /* The supervisor judges the reply it sees forwarded, and counts its turns. */
            if (a >= 0) {
                fb->continuous[a] = fb->continuous_valid[a] ? fb->continuous[a] + nearest_turn(s->position, fb->sampled[a]) : s->position;
                fb->continuous_valid[a] = true;
                fb->sampled[a] = s->position;
                fb->seen[a] = true;
                fb->healthy[a] = s->error == 0 && s->temperature_c < 60 && s->voltage_raw >= 90 && s->voltage_raw <= 126;
                fb->feedback_age_ms[a] = 0;
            }
            reply(fb, id, s->error, regs, HX_TELEMETRY_LEN, SERVO_TURNAROUND_MS);
        } else if (addr == HX_REG_MODE && width == 1) {
            regs[0] = s->mode;
            reply(fb, id, 0, regs, 1, SERVO_TURNAROUND_MS);
        } else if (addr == HX_REG_TORQUE && width == 1) {
            regs[0] = s->torque;
            reply(fb, id, 0, regs, 1, SERVO_TURNAROUND_MS);
        } else if (addr == HX_REG_PWM && width == 2) {
            regs[0] = (uint8_t)s->pwm_raw; regs[1] = (uint8_t)(s->pwm_raw >> 8);
            reply(fb, id, 0, regs, 2, SERVO_TURNAROUND_MS);
        }
        return;
    }
    if (inst == HX_WRITE) {
        if (p[0] == HX_REG_TORQUE && n == 2) { s->torque = p[1]; s->torque_writes++; }
        if (p[0] == HX_REG_PWM && n == 3) { s->pwm_raw = (uint16_t)(p[1] | p[2] << 8); s->pwm_writes++; }
        if (p[0] == HX_REG_MODE && n == 2) { if (!s->lock) s->mode = p[1]; s->mode_writes++; }
        if (p[0] == 0x37 && n == 2) s->lock = p[1];
        if (p[0] == 0x2A && n >= 3) { s->goal = (uint16_t)(p[1] | p[2] << 8); s->goal_speed = n == 7 ? (uint16_t)(p[5] | p[6] << 8) : 1000; s->goal_writes++; }
        if (p[0] == 0x2E && n == 3) {
            uint16_t v = (uint16_t)(p[1] | p[2] << 8);
            s->speed_goal = (int16_t)((v & 0x8000) ? -(v & 0x7fff) : (v & 0x7fff));
            s->speed_writes++;
        }
        reply(fb, id, 0, NULL, 0, SERVO_TURNAROUND_MS);
    }
}

void fb_rx(fb_t *fb, const uint8_t *bytes, size_t n)
{
    for (size_t i = 0; i < n; i++) {
        size_t len = hx_framer_push(&fb->framer, bytes[i]);
        if (!len) continue;
        uint8_t f[HX_MAX_FRAME];
        memcpy(f, fb->framer.buf, len);
        fb->framer.len = 0;
        if (f[2] == HX_BRIDGE_ID && f[4] == HX_LOCAL) local_command(fb, f + 5, len - 6);
        else forward(fb, f, len);
    }
}

size_t fb_tx(fb_t *fb, uint8_t *out, size_t cap)
{
    if (!fb->out_len || (int32_t)(fb->now_ms - fb->out_ready_ms) < 0) return 0;
    size_t n = fb->out_len < cap ? fb->out_len : cap;
    memcpy(out, fb->out, n);
    memmove(fb->out, fb->out + n, fb->out_len - n);
    fb->out_len -= n;
    return n;
}
